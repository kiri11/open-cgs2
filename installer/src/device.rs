//! A CGS2 over ADB (TCP port 5555 or USB): shell commands with their output and exit status, and
//! file pushes. The firmware's adbd always reports exit status 0, so commands echo theirs. It takes
//! messages of at most 4096 bytes and closes a stream only after its last write is acknowledged,
//! which adb_client 3.2 does not do, so this speaks the few ADB messages needed itself.
use nusb::descriptors::TransferType;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, Direction, In, Out};
use nusb::MaybeFuture;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CNXN: u32 = 0x4e58_4e43;
const AUTH: u32 = 0x4854_5541;
const OPEN: u32 = 0x4e45_504f;
const OKAY: u32 = 0x5941_4b4f;
const CLSE: u32 = 0x4553_4c43;
const WRTE: u32 = 0x4554_5257;
const MAX: u32 = 4096;

/// A byte stream to adbd. `flush` ends a USB transfer, as adbd reads each message part from its own.
trait Link: Read + Write {}
impl Link for TcpStream {}

/// ADB's bulk endpoints, read in transfers of one full payload: adbd ends a payload of 4096 bytes
/// with no short packet and waits for its OKAY, so a longer transfer would never complete.
struct Usb(EndpointRead<Bulk>, EndpointWrite<Bulk>);
impl Link for Usb {}

impl Read for Usb {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Usb {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.1.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.1.flush_end()
    }
}

pub struct Device {
    link: Box<dyn Link>,
    max: usize,
    streams: u32,
    pub host: String,
}

const STATUS: &str = "__open_cgs2_status=";
const ADB_ON: &str = "is ADB on (Settings → About → tap Device Name 7 times → Developer options)?";

impl Device {
    /// Connects to `host`: an address, `usb` for the one device on USB, or a USB serial number.
    pub fn connect(host: &str) -> Result<Device, String> {
        let link: Box<dyn Link> = match host == "usb" || usb_serials().iter().any(|s| s == host) {
            true => Box::new(open_usb(host)?),
            false => {
                let addr = address(host)?;
                let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{addr}: {e}; {ADB_ON}"))?;
                tcp.set_read_timeout(Some(Duration::from_secs(60))).and_then(|_| tcp.set_nodelay(true)).map_err(|e| e.to_string())?;
                Box::new(tcp)
            }
        };
        let mut d = Device { link, max: MAX as usize, streams: 0, host: host.into() };
        d.send(CNXN, 0x0100_0000, MAX, b"host::\0")?;
        let mut signed = false;
        loop {
            let (cmd, arg0, arg1, data) = d.recv()?;
            match cmd {
                CNXN => return Ok(Device { max: arg1.min(MAX) as usize, ..d }),
                AUTH if arg0 == 1 && !signed => {
                    d.send(AUTH, 2, 0, &crate::key::sign(&data)?)?;
                    signed = true;
                }
                AUTH => return Err(format!("{host}: ADB does not accept this PC's key ({})", crate::key::path().display())),
                _ => {}
            }
        }
    }

    fn send(&mut self, cmd: u32, arg0: u32, arg1: u32, data: &[u8]) -> Result<(), String> {
        let sum = data.iter().map(|&b| b as u32).sum::<u32>();
        let msg: Vec<u8> = [cmd, arg0, arg1, data.len() as u32, sum, !cmd].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut write = |b: &[u8]| self.link.write_all(b).and_then(|_| self.link.flush());
        write(&msg).and_then(|_| if data.is_empty() { Ok(()) } else { write(data) }).map_err(|e| format!("{}: {e}", self.host))
    }

    fn recv(&mut self) -> Result<(u32, u32, u32, Vec<u8>), String> {
        let mut h = [0u8; 24];
        self.link.read_exact(&mut h).map_err(|e| format!("{}: {e}", self.host))?;
        let word = |i: usize| u32::from_le_bytes(h[i * 4..i * 4 + 4].try_into().unwrap());
        let mut data = vec![0; word(3) as usize];
        self.link.read_exact(&mut data).map_err(|e| format!("{}: {e}", self.host))?;
        Ok((word(0), word(1), word(2), data))
    }

    /// Opens `service`, writes `input` to it and returns what it writes back until it closes.
    fn call(&mut self, service: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        self.streams += 1;
        let local = self.streams;
        self.send(OPEN, local, 0, format!("{service}\0").as_bytes())?;
        let mut chunks = input.chunks(self.max);
        let (mut opened, mut out) = (false, Vec::new());
        loop {
            let (cmd, remote, to, data) = self.recv()?;
            if to != local {
                continue;
            }
            match cmd {
                OKAY => {
                    opened = true;
                    if let Some(c) = chunks.next() {
                        self.send(WRTE, local, remote, c)?;
                    }
                }
                WRTE => {
                    out.extend(data);
                    self.send(OKAY, local, remote, &[])?;
                }
                CLSE if opened => return Ok(out),
                CLSE => return Err(format!("{}: ADB refused {service}", self.host)),
                _ => {}
            }
        }
    }

    /// Runs `cmd` with sh and returns its output; an exit status other than 0 is an error.
    pub fn sh(&mut self, cmd: &str) -> Result<String, String> {
        let out = self.call(&format!("shell:{cmd} 2>&1; echo {STATUS}$?"), &[])?;
        let text = String::from_utf8_lossy(&out).replace("\r\n", "\n");
        let (output, status) = text.rsplit_once(STATUS).ok_or_else(|| format!("{}: no answer to {cmd:?}", self.host))?;
        match status.trim() {
            "0" => Ok(output.to_string()),
            s => Err(format!("{cmd:?} failed ({s}): {}", output.trim())),
        }
    }

    /// Whether `cmd` succeeds.
    pub fn test(&mut self, cmd: &str) -> bool {
        self.sh(cmd).is_ok()
    }

    pub fn push(&mut self, data: &[u8], path: &str, mode: u32) -> Result<(), String> {
        let dir = path.rsplit_once('/').map_or("/", |(d, _)| d);
        self.sh(&format!("mkdir -p '{dir}'"))?;
        let tmp = format!("{path}.new");
        let reply = self.call("sync:", &sync_send(data, &format!("{tmp},{}", 0o100000 | mode)))?;
        if !reply.starts_with(b"OKAY") {
            return Err(format!("{path}: {}", String::from_utf8_lossy(reply.get(8..).unwrap_or_default())));
        }
        self.sh(&format!("chmod {mode:o} '{tmp}' && mv '{tmp}' '{path}'")).map(drop)
    }

    pub fn pull(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let request = [&b"RECV"[..], &(path.len() as u32).to_le_bytes(), path.as_bytes(), b"QUIT\0\0\0\0"].concat();
        let reply = self.call("sync:", &request)?;
        let (mut i, mut out) = (0, Vec::new());
        while let Some(head) = reply.get(i..i + 8) {
            let n = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
            let body = reply.get(i + 8..i + 8 + n).unwrap_or_default();
            match &head[..4] {
                b"DATA" => out.extend(body),
                b"DONE" => return Ok(out),
                _ => return Err(format!("{path}: {}", String::from_utf8_lossy(body))),
            }
            i += 8 + n;
        }
        Err(format!("{path}: the device stopped sending"))
    }
}

/// The sync protocol's SEND of one file, in DATA blocks of at most 64 KiB, then QUIT.
fn sync_send(data: &[u8], spec: &str) -> Vec<u8> {
    let head = |id: &[u8; 4], n: usize| [&id[..], &(n as u32).to_le_bytes()].concat();
    let mtime = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as usize);
    let mut s = [head(b"SEND", spec.len()), spec.as_bytes().to_vec()].concat();
    for c in data.chunks(64 * 1024) {
        s.extend(head(b"DATA", c.len()));
        s.extend(c);
    }
    s.extend(head(b"DONE", mtime));
    s.extend(head(b"QUIT", 0));
    s
}

fn address(host: &str) -> Result<SocketAddr, String> {
    let with_port = if host.contains(':') { host.to_string() } else { format!("{host}:5555") };
    with_port.to_socket_addrs().map_err(|e| format!("{host}: {e}"))?.next().ok_or_else(|| format!("{host}: no address"))
}

fn adb_interface(i: &nusb::InterfaceInfo) -> bool {
    (i.class(), i.subclass(), i.protocol()) == (0xff, 0x42, 0x01)
}

fn usb_devices() -> Vec<nusb::DeviceInfo> {
    nusb::list_devices().wait().map_or(Vec::new(), |l| l.filter(|d| d.interfaces().any(adb_interface)).collect())
}

/// Serial numbers of the devices with ADB on USB.
pub fn usb_serials() -> Vec<String> {
    usb_devices().iter().map(|d| d.serial_number().unwrap_or_default().to_string()).collect()
}

/// Claims the ADB interface of the device with serial number `host`, or of the only one for `usb`.
fn open_usb(host: &str) -> Result<Usb, String> {
    let found: Vec<_> = usb_devices().into_iter().filter(|d| host == "usb" || d.serial_number() == Some(host)).collect();
    let info = match &found[..] {
        [d] => d,
        [] => return Err(format!("no device with ADB on USB; {ADB_ON}")),
        _ => return Err(format!("several devices on USB; name one: {}", usb_serials().join(", "))),
    };
    let fail = |e: nusb::Error| format!("{host}: {e}; is Google's adb running (adb kill-server)?");
    let number = info.interfaces().find(|i| adb_interface(i)).unwrap().interface_number();
    let interface = info.open().wait().and_then(|d| d.claim_interface(number).wait()).map_err(fail)?;
    let bulk = |dir: Direction| {
        let eps = interface.descriptor().into_iter().flat_map(|d| d.endpoints().collect::<Vec<_>>());
        eps.into_iter().find(|e| e.transfer_type() == TransferType::Bulk && e.direction() == dir).map(|e| e.address()).ok_or(format!("{host}: no ADB endpoints"))
    };
    let mut read = interface.endpoint::<Bulk, In>(bulk(Direction::In)?).map_err(fail)?.reader(MAX as usize);
    read.set_read_timeout(Duration::from_secs(60));
    let mut write = interface.endpoint::<Bulk, Out>(bulk(Direction::Out)?).map_err(fail)?.writer(MAX as usize);
    write.set_write_timeout(Duration::from_secs(60));
    Ok(Usb(read, write))
}

/// Hosts on this PC's /24 with ADB's port open, the likely CGS2s.
pub fn scan() -> Vec<String> {
    let Some(local) = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr())).ok() else {
        return Vec::new();
    };
    let std::net::IpAddr::V4(ip) = local.ip() else { return Vec::new() };
    let [a, b, c, _] = ip.octets();
    let threads: Vec<_> = (1..255u8)
        .map(|d| {
            std::thread::spawn(move || {
                let addr = SocketAddr::from(([a, b, c, d], 5555));
                TcpStream::connect_timeout(&addr, Duration::from_millis(400)).ok().map(|_| format!("{a}.{b}.{c}.{d}"))
            })
        })
        .collect();
    threads.into_iter().filter_map(|t| t.join().ok().flatten()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Stands in for the firmware's adbd: messages of at most 4096 bytes, and a stream closes only
    /// once its last write is acknowledged. Returns what was written to sync streams, and sends
    /// `file` for a RECV.
    fn adbd(listener: TcpListener, file: Vec<u8>) -> Vec<u8> {
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut d = Device { link: Box::new(tcp), max: MAX as usize, streams: 100, host: "adbd".into() };
        let mut synced = Vec::new();
        while let Ok((cmd, from, to, data)) = d.recv() {
            assert!(data.len() <= MAX as usize, "{} bytes", data.len());
            match cmd {
                CNXN => d.send(CNXN, 0x0100_0000, MAX, b"device::\0").unwrap(),
                OPEN => {
                    d.streams += 1;
                    d.send(OKAY, d.streams, from, &[]).unwrap();
                    if data.starts_with(b"shell:") {
                        d.send(WRTE, d.streams, from, format!("{STATUS}0\n").as_bytes()).unwrap();
                    }
                }
                WRTE => {
                    synced.extend(&data);
                    d.send(OKAY, to, from, &[]).unwrap();
                    if synced.ends_with(b"QUIT\0\0\0\0") {
                        let reply = match synced.starts_with(b"RECV") {
                            true => [&b"DATA"[..], &(file.len() as u32).to_le_bytes(), &file, b"DONE\0\0\0\0"].concat(),
                            false => b"OKAY\0\0\0\0".to_vec(),
                        };
                        for c in reply.chunks(MAX as usize) {
                            d.send(WRTE, to, from, c).unwrap();
                        }
                    }
                }
                OKAY => drop(d.send(CLSE, to, from, &[])),
                _ => {}
            }
        }
        synced
    }

    #[test]
    fn pushes_in_messages_adbd_takes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || adbd(listener, Vec::new()));
        let data: Vec<u8> = (0..100_000u32).map(|i| i as u8).collect();
        Device::connect(&addr).unwrap().push(&data, "/tmp/f", 0o644).unwrap();
        let synced = server.join().unwrap();
        let word = |i: usize| u32::from_le_bytes(synced[i..i + 4].try_into().unwrap()) as usize;
        let (mut i, mut got) = (8 + word(4), Vec::new());
        while &synced[i..i + 4] == b"DATA" {
            got.extend(&synced[i + 8..i + 8 + word(i + 4)]);
            i += 8 + word(i + 4);
        }
        assert_eq!((&synced[8..8 + word(4)], got), (&b"/tmp/f.new,33188"[..], data));
    }

    #[test]
    fn pulls_a_file_sent_in_several_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let file: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        let sent = file.clone();
        let server = std::thread::spawn(move || adbd(listener, sent));
        assert_eq!(Device::connect(&addr).unwrap().pull("/tmp/f").unwrap(), file);
        assert!(server.join().unwrap().starts_with(b"RECV\x06\0\0\0/tmp/f"));
    }
}
