//! A CGS2 over ADB (TCP port 5555): shell commands with their output and exit status, and file
//! pushes. The firmware's adbd always reports exit status 0, so commands echo theirs.
use adb_client::tcp::ADBTcpDevice;
use adb_client::ADBDeviceExt;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct Device {
    adb: ADBTcpDevice,
    pub host: String,
}

const STATUS: &str = "__open_cgs2_status=";

impl Device {
    pub fn connect(host: &str) -> Result<Device, String> {
        let addr = address(host)?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{addr}: {e}; is ADB on (Settings → About → tap Device Name 7 times → Developer options)?"))?;
        let adb = ADBTcpDevice::new_with_custom_private_key(addr, crate::key::path()).map_err(|e| format!("{addr}: {e}"))?;
        Ok(Device { adb, host: host.into() })
    }

    /// Runs `cmd` with sh and returns its output; an exit status other than 0 is an error.
    pub fn sh(&mut self, cmd: &str) -> Result<String, String> {
        let mut out = Vec::new();
        self.adb
            .shell_command(&format!("{cmd} 2>&1; echo {STATUS}$?"), Some(&mut out), None)
            .map_err(|e| format!("{}: {e}", self.host))?;
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
        self.adb.push(&mut &data[..], &tmp).map_err(|e| format!("{path}: {e}"))?;
        self.sh(&format!("chmod {mode:o} '{tmp}' && mv '{tmp}' '{path}'")).map(drop)
    }
}

fn address(host: &str) -> Result<SocketAddr, String> {
    let with_port = if host.contains(':') { host.to_string() } else { format!("{host}:5555") };
    with_port.to_socket_addrs().map_err(|e| format!("{host}: {e}"))?.next().ok_or_else(|| format!("{host}: no address"))
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
