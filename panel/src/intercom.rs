//! Intercom between units: holding Talk streams the microphone to every peer, and streams from
//! peers play on the speaker. Units sharing the household key that the installer writes into
//! each config find each other with signed UDP broadcasts on the same port, and only signed
//! streams from known peers are played. Peers can also be listed by address.
use crate::{audio, ha::Cmd, log, screen, Shared};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsRawFd;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const FRAME: usize = 320;
const PREBUFFER: usize = 960;
const CAP: usize = 30 * 16000;
const TICK: Duration = Duration::from_millis(20);
const KEEP_ALIVE: Duration = Duration::from_secs(3);
const LINK_CAP: usize = 150;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(1);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(6);
const INITIAL_GAIN: f32 = 4.0;
const MIN_GAIN: f32 = 1.0;
const MAX_GAIN: f32 = 16.0;
const TARGET_PEAK: f32 = 16000.0;
const PEAK_LIMIT: f32 = 30000.0;
const MIN_SPEECH_RMS: f32 = 100.0;
const SPEECH_RATIO: f32 = 3.0;
const NOISE_RISE: f32 = 0.02;
const GAIN_RISE: f32 = 1.02;
const HEADER: &[u8] = b"CGS2-INTERCOM 1\n";
const HELLO_EVERY: Duration = Duration::from_secs(10);
/// A discovered peer is dropped after this long without a hello.
const PEER_TTL: Duration = Duration::from_secs(35);
/// Signed headers and hellos are accepted this far from the local clock, in seconds.
const CLOCK_SLACK: i64 = 300;

struct Agc {
    g: f32,
    noise_floor: f32,
}

impl Default for Agc {
    fn default() -> Agc {
        Agc { g: INITIAL_GAIN, noise_floor: f32::INFINITY }
    }
}

impl Agc {
    fn apply(&mut self, pcm: &mut [u8]) {
        let count = pcm.len() / 2;
        if count == 0 {
            return;
        }
        let mut energy = 0.0;
        let mut peak = 0.0f32;
        for pair in pcm.chunks_exact(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]) as f32;
            energy += sample * sample;
            peak = peak.max(sample.abs());
        }
        let rms = (energy / count as f32).sqrt();
        let speech = rms > MIN_SPEECH_RMS && rms > SPEECH_RATIO * self.noise_floor;
        if rms < self.noise_floor {
            self.noise_floor = rms;
        } else if !speech {
            // Speech must not become its own noise floor during a held press.
            self.noise_floor += NOISE_RISE * (rms - self.noise_floor);
        }
        if speech {
            self.g = self.g.max((self.g * GAIN_RISE).min(TARGET_PEAK / peak));
        }
        if peak * self.g > PEAK_LIMIT {
            self.g = PEAK_LIMIT / peak;
        }
        self.g = self.g.clamp(MIN_GAIN, MAX_GAIN);
        for pair in pcm.chunks_exact_mut(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]) as f32 * self.g;
            // Full-scale input exceeds the ceiling even at the minimum gain.
            pair.copy_from_slice(&(sample.clamp(-PEAK_LIMIT, PEAK_LIMIT) as i16).to_le_bytes());
        }
    }
}

pub struct Intercom {
    pub name: String,
    pub port: u16,
    /// The household key; without one only the listed peers are used and streams are unsigned.
    pub key: Option<Vec<u8>>,
    pub peers: Vec<Peer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub name: String,
    pub host: IpAddr,
}

/// The first 32 hex digits of HMAC-SHA256.
fn sign(key: &[u8], msg: &str) -> String {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let pad = |b: u8| k.iter().map(|x| x ^ b).collect::<Vec<u8>>();
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(msg.as_bytes()).finalize();
    let outer = Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize();
    outer[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn epoch() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn header(key: Option<&[u8]>, t: i64) -> Vec<u8> {
    match key {
        Some(k) => format!("CGS2-INTERCOM 2 {t} {}\n", sign(k, &format!("talk {t}"))).into_bytes(),
        None => HEADER.to_vec(),
    }
}

fn hello(key: &[u8], t: i64, id: u64, name: &str) -> String {
    format!("CGS2-HELLO {t} {id} {} {name}", sign(key, &format!("hello {t} {id} {name}")))
}

/// The sender's id and name, if the hello is signed with `key` and recent.
fn parse_hello(key: &[u8], text: &str, now: i64) -> Option<(u64, String)> {
    let mut parts = text.strip_prefix("CGS2-HELLO ")?.splitn(4, ' ');
    let (t, id, mac, name) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<u64>().ok()?, parts.next()?, parts.next()?);
    ((t - now).abs() <= CLOCK_SLACK && mac == sign(key, &format!("hello {t} {id} {name}")) && !name.is_empty()).then(|| (id, name.to_string()))
}

impl Intercom {
    pub fn from_json(v: &Value, default_name: &str) -> Option<Intercom> {
        let name = v["name"].as_str().unwrap_or(default_name).to_string();
        let key = v["key"].as_str().filter(|k| k.len() >= 16).map(|k| k.as_bytes().to_vec());
        let peers = v["peers"].as_array().map_or_else(Vec::new, |a| {
            a.iter().filter_map(|p| {
                let peer_name = p["name"].as_str()?;
                if peer_name == name {
                    return None;
                }
                match p["host"].as_str().unwrap_or_default().parse() {
                    Ok(host) => Some(Peer { name: peer_name.into(), host }),
                    Err(e) => { log(format!("intercom {peer_name}: bad host: {e}")); None }
                }
            }).collect::<Vec<_>>()
        });
        if peers.is_empty() && key.is_none() {
            return None;
        }
        Some(Intercom { name, port: v["port"].as_u64().and_then(|n| u16::try_from(n).ok()).filter(|&n| n > 0).unwrap_or(8098), key, peers })
    }
}

#[derive(Default)]
pub struct Status {
    pub talking: bool,
    generation: u64,
    links: Vec<(String, &'static str)>,
    pub incoming: Vec<String>,
    /// Listed peers, then the ones found on the network.
    pub peers: Vec<Peer>,
}

pub fn snapshot(s: &Status) -> Value {
    (!s.peers.is_empty()).then(|| json!({
        "peers": s.peers.iter().map(|p| &p.name).collect::<Vec<_>>(),
        "talking": s.talking,
        "links": s.links.iter().map(|(name, state)| json!({"name": name, "state": state})).collect::<Vec<_>>(),
        "incoming": s.incoming,
    })).unwrap_or(Value::Null)
}

struct Stream {
    name: String,
    samples: VecDeque<i16>,
    open: bool,
    started: bool,
    received: usize,
    underruns: usize,
    duration: Duration,
}

/// Queues independent streams, then sums one 20 ms frame when asked.
#[derive(Default)]
struct Mixer {
    streams: BTreeMap<u64, Stream>,
    ended: Vec<Stream>,
}

impl Mixer {
    fn add(&mut self, id: u64, name: String) {
        self.streams.insert(id, Stream { name, samples: VecDeque::new(), open: true, started: false, received: 0, underruns: 0, duration: Duration::ZERO });
    }

    fn push(&mut self, id: u64, samples: impl IntoIterator<Item = i16>) {
        if let Some(s) = self.streams.get_mut(&id) {
            for sample in samples {
                s.received += 1;
                if s.samples.len() == CAP {
                    s.samples.pop_front();
                }
                s.samples.push_back(sample);
            }
            s.started |= s.samples.len() >= PREBUFFER;
        }
    }

    fn close(&mut self, id: u64, duration: Duration) {
        if let Some(s) = self.streams.get_mut(&id) {
            s.open = false;
            s.duration = duration;
            s.started = true;
        }
        self.prune();
    }

    fn prune(&mut self) {
        let ended: Vec<_> = self.streams.iter().filter(|(_, s)| !s.open && s.samples.is_empty()).map(|(&id, _)| id).collect();
        for id in ended {
            self.ended.push(self.streams.remove(&id).unwrap());
        }
    }

    fn ready(&self) -> bool {
        self.streams.values().any(|s| s.started)
    }

    fn present(&self) -> Vec<String> {
        let mut names: Vec<_> = self.streams.values().map(|s| s.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    fn frame(&mut self) -> Option<[i16; FRAME]> {
        for s in self.streams.values_mut() {
            if s.open && s.samples.len() < FRAME {
                s.underruns += 1;
            }
        }
        if !self.ready() {
            return None;
        }
        let mut sum = [0i32; FRAME];
        for s in self.streams.values_mut().filter(|s| s.started) {
            for n in &mut sum {
                *n = n.saturating_add(s.samples.pop_front().unwrap_or(0) as i32);
            }
        }
        self.prune();
        Some(sum.map(|n| n.clamp(i16::MIN as i32, i16::MAX as i32) as i16))
    }
}

struct Incoming {
    mixer: Mutex<Mixer>,
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    next_id: AtomicU64,
}

impl Incoming {
    fn change<R>(&self, f: impl FnOnce(&mut Mixer) -> R) -> R {
        let mut m = self.mixer.lock().unwrap();
        let r = f(&mut m);
        for s in m.ended.drain(..) {
            log(format!("intercom incoming {} ended: {:.2}s, {} samples, {} underruns", s.name, s.duration.as_secs_f64(), s.received, s.underruns));
        }
        let names = m.present();
        if self.shared.read(|s| s.intercom.incoming != names) {
            self.shared.update(|s| s.intercom.incoming = names);
        }
        r
    }

    fn receive(&self, mut socket: TcpStream, peers: &[Peer], key: Option<&[u8]>) {
        let Ok(addr) = socket.peer_addr() else { return };
        let Some(peer) = peers.iter().find(|p| p.host == addr.ip()) else {
            log(format!("intercom rejected {addr}: not a peer"));
            return;
        };
        if let Err(e) = read_header(&mut socket, key) {
            log(format!("intercom rejected {}: {e}", peer.name));
            return;
        }
        let started = Instant::now();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.change(|m| m.add(id, peer.name.clone()));
        log(format!("intercom incoming {} started", peer.name));
        screen::set(&self.shared, true, "intercom");
        if self.shared.read(|s| s.phase == "listening") {
            let _ = self.tx.send(Cmd::Dismiss);
        }
        let mut buf = [0u8; 4096];
        let mut odd = None;
        loop {
            let n = match socket.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut samples = Vec::with_capacity((n + 1) / 2);
            let mut i = 0;
            if let Some(lo) = odd.take() {
                samples.push(i16::from_le_bytes([lo, buf[0]]));
                i = 1;
            }
            for pair in buf[i..n].chunks_exact(2) {
                samples.push(i16::from_le_bytes([pair[0], pair[1]]));
            }
            if (n - i) % 2 != 0 {
                odd = Some(buf[n - 1]);
            }
            self.change(|m| m.push(id, samples));
        }
        self.change(|m| m.close(id, started.elapsed()));
    }
}

fn signed(key: &[u8], header: &[u8]) -> bool {
    let text = String::from_utf8_lossy(header);
    let Some((t, mac)) = text.trim_end().strip_prefix("CGS2-INTERCOM 2 ").and_then(|r| r.split_once(' ')) else { return false };
    t.parse::<i64>().is_ok_and(|t| (t - epoch()).abs() <= CLOCK_SLACK && mac == sign(key, &format!("talk {t}")))
}

fn read_header(socket: &mut TcpStream, key: Option<&[u8]>) -> io::Result<()> {
    let until = Instant::now() + Duration::from_secs(2);
    let mut header = Vec::new();
    for _ in 0..64 {
        let left = until.checked_duration_since(Instant::now()).ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "header timeout"))?;
        socket.set_read_timeout(Some(left))?;
        let mut byte = [0];
        socket.read_exact(&mut byte)?;
        header.push(byte[0]);
        if byte[0] == b'\n' {
            if !key.map_or(header == HEADER, |k| signed(k, &header)) {
                break;
            }
            socket.set_read_timeout(Some(READ_TIMEOUT))?;
            return Ok(());
        }
    }
    Err(io::Error::new(io::ErrorKind::InvalidData, "bad header"))
}

struct LinkQueue {
    chunks: VecDeque<Vec<u8>>,
    open: bool,
    dropped: usize,
}

struct Link {
    queue: Arc<(Mutex<LinkQueue>, Condvar)>,
}

impl Link {
    fn new() -> Link {
        Link { queue: Arc::new((Mutex::new(LinkQueue { chunks: VecDeque::new(), open: true, dropped: 0 }), Condvar::new())) }
    }

    fn push(&self, pcm: Vec<u8>) {
        let (m, c) = &*self.queue;
        let mut q = m.lock().unwrap();
        if q.chunks.len() == LINK_CAP {
            q.chunks.pop_front();
            q.dropped += 1;
        }
        q.chunks.push_back(pcm);
        c.notify_all();
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.queue.0.lock().unwrap().open = false;
        self.queue.1.notify_all();
    }
}

#[derive(Default)]
struct LinkStats {
    connected: Duration,
    bytes: usize,
    reconnects: usize,
    longest_write: Duration,
}

struct LinkWriter<'a> {
    socket: &'a mut TcpStream,
    bytes: &'a mut usize,
}

impl Write for LinkWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.socket.write(bytes)?;
        *self.bytes += n;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> { self.socket.flush() }
}

impl LinkStats {
    fn write(&mut self, socket: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
        let started = Instant::now();
        let result = LinkWriter { socket, bytes: &mut self.bytes }.write_all(bytes);
        self.longest_write = self.longest_write.max(started.elapsed());
        result
    }
}

fn send_link(peer: &Peer, port: u16, key: Option<&[u8]>, queue: Arc<(Mutex<LinkQueue>, Condvar)>, status: impl Fn(&'static str)) {
    let (m, c) = &*queue;
    let current = Mutex::new(None::<TcpStream>);
    let done = AtomicBool::new(false);
    let mut stats = LinkStats::default();
    let mut live = false;
    thread::scope(|scope| {
        scope.spawn(|| {
            let q = c.wait_while(m.lock().unwrap(), |q| q.open && !done.load(Ordering::SeqCst)).unwrap();
            let (q, _) = c.wait_timeout_while(q, DRAIN_TIMEOUT, |_| !done.load(Ordering::SeqCst)).unwrap();
            if !done.load(Ordering::SeqCst) {
                // Release must also bound a write that was already blocked.
                if let Some(socket) = current.lock().unwrap().as_ref() {
                    let _ = socket.shutdown(Shutdown::Write);
                }
            }
            drop(q);
        });
        while m.lock().unwrap().open {
            let mut connected = None;
            let result = (|| -> io::Result<()> {
                let addr = SocketAddr::new(peer.host, port);
                let mut socket = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
                socket.set_nodelay(true)?;
                socket.set_write_timeout(Some(WRITE_TIMEOUT))?;
                {
                    let q = m.lock().unwrap();
                    if !q.open {
                        return Ok(());
                    }
                    *current.lock().unwrap() = Some(socket.try_clone()?);
                }
                connected = Some(Instant::now());
                stats.write(&mut socket, &header(key, epoch()))?;
                live = true;
                status("live");
                loop {
                    let pcm = {
                        let mut q = c.wait_while(m.lock().unwrap(), |q| q.open && q.chunks.is_empty()).unwrap();
                        q.chunks.pop_front()
                    };
                    let Some(pcm) = pcm else { break };
                    stats.write(&mut socket, &pcm)?;
                }
                socket.shutdown(Shutdown::Write)
            })();
            if let Some(at) = connected {
                stats.connected += at.elapsed();
            }
            current.lock().unwrap().take();
            if let Err(e) = result {
                log(format!("intercom link {}: {e}", peer.name));
                status(if live { "connecting" } else { "failed" });
            }
            let (q, _) = c.wait_timeout_while(m.lock().unwrap(), RETRY, |q| q.open).unwrap();
            if !q.open {
                break;
            }
            stats.reconnects += 1;
        }
        let _q = m.lock().unwrap();
        done.store(true, Ordering::SeqCst);
        c.notify_all();
    });
    let dropped = m.lock().unwrap().dropped;
    log(format!("intercom link {} ended: {:.2}s connected, {} bytes sent, {} reconnects, {} dropped chunks, {}ms longest write", peer.name, stats.connected.as_secs_f64(), stats.bytes, stats.reconnects, dropped, stats.longest_write.as_millis()));
}

struct Output {
    child: Child,
    input: Option<ChildStdin>,
    _hold: audio::SpeakerHold,
    idle: Option<Instant>,
}

impl Output {
    fn new(hold: audio::SpeakerHold) -> io::Result<Output> {
        let mut child = Command::new("aplay")
            .args(["-q", "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1", "-B", "100000", "-F", "20000"])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        let input = child.stdin.take().unwrap();
        let fd = input.as_raw_fd();
        // A wedged player must not block release or the watchdog behind a pipe write.
        if unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
            let e = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok(Output { child, input: Some(input), _hold: hold, idle: None })
    }

    fn finish(mut self) {
        self.input.take();
        let until = Instant::now() + Duration::from_millis(500);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn write(&mut self, frame: [i16; FRAME]) -> io::Result<()> {
        let bytes: Vec<_> = frame.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.input.as_mut().unwrap().write_all(&bytes)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Talk {
    generation: u64,
    deadline: Option<Instant>,
    agc: Agc,
    links: Vec<Link>,
    hold: Option<audio::SpeakerHold>,
    output: Option<Output>,
}

pub struct Service {
    shared: Arc<Shared>,
    tx: Sender<Cmd>,
    mic: audio::Mic,
    talk: Mutex<Talk>,
    incoming: Arc<Incoming>,
}

impl Service {
    pub fn start(shared: Arc<Shared>, tx: Sender<Cmd>) -> Arc<Service> {
        let incoming = Arc::new(Incoming { mixer: Mutex::new(Mixer::default()), shared: shared.clone(), tx: tx.clone(), next_id: AtomicU64::new(1) });
        let service: Arc<Service> = Arc::new_cyclic(|weak: &std::sync::Weak<Service>| {
            let weak = weak.clone();
            Service {
                mic: audio::Mic::new(shared.cfg.mic.clone(), move |mut pcm| {
                    let Some(s) = weak.upgrade() else { return false };
                    let mut t = s.talk.lock().unwrap();
                    if t.deadline.is_none() {
                        return true;
                    }
                    t.agc.apply(&mut pcm);
                    for link in &t.links {
                        link.push(pcm.clone());
                    }
                    true
                }),
                shared: shared.clone(), tx: tx.clone(), talk: Mutex::new(Talk::default()), incoming: incoming.clone(),
            }
        });
        let cfg = shared.cfg.intercom.as_ref().unwrap();
        shared.update(|s| s.intercom.peers = cfg.peers.clone());
        match TcpListener::bind(("0.0.0.0", cfg.port)) {
            Ok(listener) => {
                log(format!("intercom {} listening on 0.0.0.0:{}", cfg.name, cfg.port));
                let shared = shared.clone();
                thread::spawn(move || {
                    for socket in listener.incoming() {
                        match socket {
                            Ok(socket) => {
                                let (incoming, peers) = (incoming.clone(), shared.read(|s| s.intercom.peers.clone()));
                                let key = shared.cfg.intercom.as_ref().and_then(|c| c.key.clone());
                                thread::spawn(move || incoming.receive(socket, &peers, key.as_deref()));
                            }
                            Err(e) => { log(format!("intercom accept: {e}")); thread::sleep(Duration::from_secs(1)); }
                        }
                    }
                });
            }
            Err(e) => log(format!("intercom listen: {e}")),
        }
        if cfg.key.is_some() {
            let shared = shared.clone();
            thread::spawn(move || discover(&shared));
        }
        let s = service.clone();
        thread::spawn(move || s.playback());
        let s = service.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(100));
            let mut t = s.talk.lock().unwrap();
            if t.deadline.is_some_and(|at| Instant::now() >= at) {
                s.stop(&mut t, "keep-alive expired");
            }
        });
        service
    }

    pub fn talk(&self) {
        let mut t = self.talk.lock().unwrap();
        if t.deadline.is_some() {
            t.deadline = Some(Instant::now() + KEEP_ALIVE);
            return;
        }
        t.hold = Some(audio::hold_speaker());
        t.output.take();
        t.generation += 1;
        t.deadline = Some(Instant::now() + KEEP_ALIVE);
        let cfg = self.shared.cfg.intercom.as_ref().unwrap();
        log(format!("intercom {} talking", cfg.name));
        let peers = self.shared.read(|s| s.intercom.peers.clone());
        self.shared.update(|s| {
            s.intercom.talking = true;
            s.intercom.generation = t.generation;
            s.intercom.links = peers.iter().map(|p| (p.name.clone(), "connecting")).collect();
        });
        let _ = self.tx.send(Cmd::Dismiss);
        for (index, peer) in peers.into_iter().enumerate() {
            let link = Link::new();
            let queue = link.queue.clone();
            t.links.push(link);
            let (shared, generation, port, key) = (self.shared.clone(), t.generation, cfg.port, cfg.key.clone());
            thread::spawn(move || send_link(&peer, port, key.as_deref(), queue, |state| link_status(&shared, generation, index, state)));
        }
        self.mic.start();
    }

    pub fn release(&self) {
        self.stop(&mut self.talk.lock().unwrap(), "released");
    }

    fn stop(&self, t: &mut Talk, why: &str) {
        if t.deadline.take().is_none() {
            return;
        }
        self.mic.stop();
        t.links.clear();
        if self.incoming.mixer.lock().unwrap().streams.is_empty() {
            t.hold.take();
        }
        self.shared.update(|s| { s.intercom.talking = false; s.intercom.links.clear(); });
        log(format!("intercom stopped talking ({why}), gain {:.2}", t.agc.g));
    }

    fn playback(&self) {
        let mut next = Instant::now();
        let mut retry = Instant::now();
        loop {
            {
                let mut t = self.talk.lock().unwrap();
                if t.deadline.is_none() {
                    let (present, ready) = {
                        let m = self.incoming.mixer.lock().unwrap();
                        (!m.streams.is_empty(), m.ready())
                    };
                    if !present {
                        t.hold.take();
                    } else if t.output.is_none() && t.hold.is_none() && Instant::now() >= retry {
                        t.hold = Some(audio::hold_speaker());
                    }
                    if t.output.is_none() && Instant::now() >= retry && ready {
                        let hold = t.hold.take().unwrap_or_else(audio::hold_speaker);
                        match Output::new(hold) {
                            Ok(output) => { t.output = Some(output); next = Instant::now(); }
                            Err(e) => { log(format!("intercom aplay: {e}")); retry = Instant::now() + Duration::from_secs(1); }
                        }
                    }
                    if let Some(out) = t.output.as_mut() {
                        let result = match out.child.try_wait() {
                            Ok(None) => match self.incoming.change(Mixer::frame) {
                                Some(frame) => { out.idle = None; out.write(frame).map(|_| false) }
                                None => {
                                    let since = out.idle.get_or_insert_with(Instant::now);
                                    Ok(since.elapsed() >= Duration::from_millis(300))
                                }
                            },
                            Ok(Some(status)) => Err(io::Error::other(format!("exited: {status}"))),
                            Err(e) => Err(e),
                        };
                        match result {
                            Ok(false) => {}
                            Ok(true) => { t.output.take().unwrap().finish(); }
                            Err(e) => {
                                log(format!("intercom aplay: {e}"));
                                t.output.take();
                                retry = Instant::now() + Duration::from_secs(1);
                            }
                        }
                    }
                }
            }
            next += TICK;
            let now = Instant::now();
            if now.saturating_duration_since(next) > Duration::from_millis(100) || next.saturating_duration_since(now) > Duration::from_millis(100) {
                next = now + TICK;
            }
            if let Some(left) = next.checked_duration_since(Instant::now()) {
                thread::sleep(left);
            }
        }
    }
}

/// Announces this unit on the LAN every HELLO_EVERY and keeps the peers that announce themselves
/// with the same key, by the address their hello came from.
fn discover(shared: &Shared) {
    let cfg = shared.cfg.intercom.as_ref().unwrap();
    let key = cfg.key.as_deref().unwrap();
    let socket = match UdpSocket::bind(("0.0.0.0", cfg.port)).and_then(|s| s.set_broadcast(true).map(|_| s)) {
        Ok(s) => s,
        Err(e) => return log(format!("intercom discovery: {e}")),
    };
    let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
    let mut b = [0u8; 8];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    let id = u64::from_le_bytes(b);
    let mut seen: BTreeMap<String, (Peer, Instant)> = BTreeMap::new();
    let mut last_hello: Option<Instant> = None;
    let mut buf = [0u8; 512];
    loop {
        if last_hello.is_none_or(|t| t.elapsed() >= HELLO_EVERY) {
            last_hello = Some(Instant::now());
            let msg = hello(key, epoch(), id, &cfg.name);
            if let Err(e) = socket.send_to(msg.as_bytes(), (Ipv4Addr::BROADCAST, cfg.port)) {
                log(format!("intercom hello: {e}"));
            }
        }
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            if let Some((other, name)) = parse_hello(key, &String::from_utf8_lossy(&buf[..n]), epoch()) {
                if other != id && name != cfg.name {
                    if !seen.contains_key(&name) {
                        log(format!("intercom found {name} at {}", from.ip()));
                    }
                    seen.insert(name.clone(), (Peer { name, host: from.ip() }, Instant::now()));
                }
            }
        }
        seen.retain(|name, (_, at)| {
            let keep = at.elapsed() < PEER_TTL;
            if !keep {
                log(format!("intercom lost {name}"));
            }
            keep
        });
        let mut peers = cfg.peers.clone();
        peers.extend(seen.values().map(|(p, _)| p.clone()).filter(|p| !cfg.peers.iter().any(|s| s.name == p.name)));
        if shared.read(|s| s.intercom.peers != peers) {
            shared.update(|s| s.intercom.peers = peers);
        }
    }
}

fn link_status(shared: &Shared, generation: u64, index: usize, state: &'static str) {
    shared.update(|s| {
        if s.intercom.talking && s.intercom.generation == generation {
            s.intercom.links[index].1 = state;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, State};
    use std::sync::mpsc::{self, Receiver};

    fn signal(peak: f32) -> Vec<u8> {
        (0..FRAME).flat_map(|n| {
            let sample = (peak * (std::f32::consts::TAU * 400.0 * n as f32 / 16000.0).sin()).round() as i16;
            sample.to_le_bytes()
        }).collect()
    }

    fn peak(pcm: &[u8]) -> i32 {
        pcm.chunks_exact(2).map(|p| (i16::from_le_bytes([p[0], p[1]]) as i32).abs()).max().unwrap_or(0)
    }

    fn quiet_agc() -> Agc {
        let mut agc = Agc::default();
        for _ in 0..5 { agc.apply(&mut signal(20.0)); }
        agc
    }

    #[test]
    fn agc_raises_quiet_speech_toward_target() {
        let mut agc = quiet_agc();
        for _ in 0..100 {
            let before = agc.g;
            let mut pcm = signal(1500.0);
            agc.apply(&mut pcm);
            assert!(agc.g >= before);
            assert!(agc.g <= before * GAIN_RISE);
            assert!(agc.g <= MAX_GAIN);
            assert!(peak(&pcm) <= PEAK_LIMIT as i32);
        }
        assert!((agc.g - TARGET_PEAK / 1500.0).abs() < 0.01);
        let mut pcm = signal(1500.0);
        agc.apply(&mut pcm);
        assert!((peak(&pcm) - TARGET_PEAK as i32).abs() <= 1);
    }

    #[test]
    fn agc_caps_gain_and_limits_a_sudden_loud_chunk() {
        let mut agc = quiet_agc();
        for _ in 0..200 {
            agc.apply(&mut signal(400.0));
            assert!((MIN_GAIN..=MAX_GAIN).contains(&agc.g));
        }
        assert_eq!(agc.g, MAX_GAIN);
        let mut pcm = signal(10000.0);
        agc.apply(&mut pcm);
        assert_eq!(agc.g, 3.0);
        assert_eq!(peak(&pcm), PEAK_LIMIT as i32);
        agc = quiet_agc();
        for _ in 0..100 { agc.apply(&mut signal(1500.0)); }
        assert!(agc.g > 10.0);
        let mut pcm: Vec<_> = [i16::MIN, i16::MAX].into_iter().cycle().take(FRAME).flat_map(i16::to_le_bytes).collect();
        agc.apply(&mut pcm);
        assert_eq!(agc.g, MIN_GAIN);
        assert_eq!(peak(&pcm), PEAK_LIMIT as i32);
        assert_eq!(&pcm[..4], &[(-30000i16).to_le_bytes(), 30000i16.to_le_bytes()].concat());
    }

    #[test]
    fn agc_does_not_raise_noise_or_silence() {
        let mut agc = quiet_agc();
        for _ in 0..100 {
            agc.apply(&mut signal(90.0));
            assert_eq!(agc.g, INITIAL_GAIN);
        }
        let mut agc = Agc::default();
        for n in 0..100 {
            agc.apply(&mut signal(if n % 2 == 0 { 500.0 } else { 600.0 }));
            assert_eq!(agc.g, INITIAL_GAIN);
        }
        let mut silence = vec![0; FRAME * 2];
        for _ in 0..100 { agc.apply(&mut silence); }
        assert!(silence.iter().all(|&b| b == 0));
        assert_eq!(agc.g, INITIAL_GAIN);
    }

    #[test]
    fn agc_noise_floor_falls_immediately_and_rises_slowly() {
        let mut agc = Agc::default();
        agc.apply(&mut [200i16.to_le_bytes(); FRAME].concat());
        assert_eq!(agc.noise_floor, 200.0);
        agc.apply(&mut [100i16.to_le_bytes(); FRAME].concat());
        assert_eq!(agc.noise_floor, 100.0);
        agc.apply(&mut [200i16.to_le_bytes(); FRAME].concat());
        assert_eq!(agc.noise_floor, 102.0);
    }

    fn mixer() -> Mixer {
        let mut m = Mixer::default();
        m.add(1, "Bedroom".into());
        m
    }

    #[test]
    fn sums_and_clamps_streams() {
        let mut m = mixer();
        m.add(2, "Living Room".into());
        let samples = [30000, -30000, 12].into_iter().cycle().take(PREBUFFER).collect::<Vec<_>>();
        m.push(1, samples.clone());
        m.push(2, samples);
        let f = m.frame().unwrap();
        assert_eq!(&f[..3], &[32767, -32768, 24]);
        assert_eq!(m.present(), ["Bedroom", "Living Room"]);
    }

    #[test]
    fn prebuffers_open_streams_and_plays_closed_short_streams() {
        let mut m = mixer();
        m.push(1, vec![7; PREBUFFER - 1]);
        assert!(m.frame().is_none());
        m.push(1, [8]);
        assert_eq!(m.frame().unwrap(), [7; FRAME]);
        let mut m = mixer();
        m.push(1, [9, 10]);
        assert!(m.frame().is_none());
        m.close(1, Duration::from_secs(1));
        let f = m.frame().unwrap();
        assert_eq!(&f[..2], &[9, 10]);
        assert!(f[2..].iter().all(|&n| n == 0));
        assert!(m.frame().is_none());
    }

    #[test]
    fn underruns_pad_without_removing_open_streams() {
        let mut m = mixer();
        m.push(1, vec![7; PREBUFFER + 1]);
        for _ in 0..3 { m.frame().unwrap(); }
        let f = m.frame().unwrap();
        assert_eq!(f[0], 7);
        assert!(f[1..].iter().all(|&n| n == 0));
        assert_eq!(m.present(), ["Bedroom"]);
        assert_eq!(m.frame().unwrap(), [0; FRAME]);
        m.push(1, [12]);
        assert_eq!(m.frame().unwrap()[0], 12);
    }

    #[test]
    fn reports_samples_and_underruns_once_after_drain() {
        let mut m = mixer();
        assert!(m.frame().is_none());
        m.push(1, vec![7; PREBUFFER + 1]);
        for _ in 0..4 { m.frame().unwrap(); }
        m.push(1, [8]);
        m.close(1, Duration::from_secs(5));
        assert!(m.ended.is_empty());
        m.frame().unwrap();
        let ended = m.ended.pop().unwrap();
        assert_eq!(ended.name, "Bedroom");
        assert_eq!(ended.duration, Duration::from_secs(5));
        assert_eq!(ended.received, PREBUFFER + 2);
        assert_eq!(ended.underruns, 2);
        assert!(m.frame().is_none());
        assert!(m.ended.is_empty());
    }

    #[test]
    fn closed_streams_drain_then_disappear() {
        let mut m = mixer();
        m.push(1, vec![7; FRAME + 1]);
        m.close(1, Duration::from_secs(1));
        assert_eq!(m.frame().unwrap(), [7; FRAME]);
        assert_eq!(m.present(), ["Bedroom"]);
        assert_eq!(m.frame().unwrap()[0], 7);
        assert!(m.present().is_empty());
        assert!(m.frame().is_none());
        m.add(2, "Living Room".into());
        m.close(2, Duration::ZERO);
        assert!(m.present().is_empty());
    }

    #[test]
    fn caps_at_thirty_seconds_and_drops_oldest_samples() {
        let mut m = mixer();
        m.push(1, [11, 12, 13]);
        m.push(1, vec![17; CAP]);
        assert_eq!(m.streams[&1].samples.len(), CAP);
        assert_eq!(m.frame().unwrap(), [17; FRAME]);
    }

    #[test]
    fn defers_all_consumption_until_frames_are_requested() {
        let mut m = mixer();
        let pcm: Vec<_> = (0..PREBUFFER + 50).map(|n| n as i16).collect();
        m.push(1, pcm.clone());
        m.close(1, Duration::from_secs(1));
        assert!(m.ready());
        assert_eq!(m.present(), ["Bedroom"]);
        assert_eq!(m.streams[&1].samples.iter().copied().collect::<Vec<_>>(), pcm);
        assert_eq!(m.frame().unwrap().to_vec(), pcm[..FRAME]);
    }

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn config_filters_self_and_bad_hosts() {
        assert!(Intercom::from_json(&Value::Null, "Kitchen").is_none());
        assert!(Intercom::from_json(&json!({"peers": [{"name": "Kitchen", "host": "127.0.0.1"}]}), "Kitchen").is_none());
        let v = json!({"peers": [{"name": "Kitchen", "host": "192.0.2.1"}, {"name": "Hall", "host": "192.0.2.2"}, {"name": "Bad", "host": "invalid"}]});
        let c = Intercom::from_json(&v, "Kitchen").unwrap();
        assert_eq!((c.name.as_str(), c.port, c.peers.len(), c.peers[0].name.as_str()), ("Kitchen", 8098, 1, "Hall"));
        let c = Intercom::from_json(&json!({"key": std::str::from_utf8(KEY).unwrap()}), "Kitchen").unwrap();
        assert!(c.peers.is_empty() && c.key.is_some());
    }

    #[test]
    fn hellos_and_headers_need_the_key_and_a_recent_time() {
        let now = epoch();
        let msg = hello(KEY, now, 7, "Living Room");
        assert_eq!(parse_hello(KEY, &msg, now), Some((7, "Living Room".into())));
        assert_eq!(parse_hello(b"another key, sixteen+", &msg, now), None);
        assert_eq!(parse_hello(KEY, &msg, now + CLOCK_SLACK + 1), None);
        assert_eq!(parse_hello(KEY, &msg.replace("Living", "Dining"), now), None);
        assert!(signed(KEY, &header(Some(KEY), now)));
        assert!(!signed(KEY, &header(Some(KEY), now - CLOCK_SLACK - 1)));
        assert!(!signed(b"another key, sixteen+", &header(Some(KEY), now)));
        assert!(header(Some(KEY), now).len() < 64);
    }

    fn receiver(peers: Vec<Peer>) -> (Arc<Incoming>, SocketAddr, thread::JoinHandle<()>, Receiver<Cmd>) {
        receiver_with(peers, None)
    }

    fn receiver_with(peers: Vec<Peer>, key: Option<&'static [u8]>) -> (Arc<Incoming>, SocketAddr, thread::JoinHandle<()>, Receiver<Cmd>) {
        let cfg = Config::build(&json!({}), std::path::Path::new("."), Value::Null);
        let shared = Arc::new(Shared { cfg, state: Mutex::new(State { phase: "idle", ..State::default() }), changed: Condvar::new() });
        let (tx, rx) = mpsc::channel();
        let incoming = Arc::new(Incoming { mixer: Mutex::new(Mixer::default()), shared, tx, next_id: AtomicU64::new(1) });
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let i = incoming.clone();
        let worker = thread::spawn(move || i.receive(listener.accept().unwrap().0, &peers, key));
        (incoming, addr, worker, rx)
    }

    #[test]
    fn plays_only_signed_streams_when_keyed() {
        for (header, played) in [(header(Some(KEY), epoch()), true), (HEADER.to_vec(), false)] {
            let (i, addr, worker, _) = receiver_with(vec![peer()], Some(KEY));
            let mut socket = TcpStream::connect(addr).unwrap();
            socket.write_all(&header).unwrap();
            socket.write_all(&[1u8; 640]).unwrap();
            drop(socket);
            worker.join().unwrap();
            assert_eq!(i.mixer.lock().unwrap().streams.len() == 1, played);
        }
    }

    fn peer() -> Peer {
        Peer { name: "Living Room".into(), host: "127.0.0.1".parse().unwrap() }
    }

    fn until(mut f: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(3);
        while !f() {
            assert!(Instant::now() < until, "timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn refuses_connections_from_non_peers() {
        let peers = vec![Peer { name: "Elsewhere".into(), host: "127.0.0.2".parse().unwrap() }];
        let (i, addr, worker, _) = receiver(peers);
        let mut socket = TcpStream::connect(addr).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        worker.join().unwrap();
        assert!(matches!(socket.read(&mut [0; 1]), Ok(0) | Err(_)));
        assert!(i.mixer.lock().unwrap().present().is_empty());
    }

    #[test]
    fn rejects_wrong_and_overlong_headers() {
        for header in [b"CGS2-INTERCOM 2\n".to_vec(), vec![b'x'; 64]] {
            let (i, addr, worker, _) = receiver(vec![peer()]);
            let mut socket = TcpStream::connect(addr).unwrap();
            socket.write_all(&header).unwrap();
            worker.join().unwrap();
            assert!(i.mixer.lock().unwrap().present().is_empty());
        }
    }

    fn sender(port: u16) -> (Link, thread::JoinHandle<()>, Receiver<&'static str>) {
        let link = Link::new();
        let queue = link.queue.clone();
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || send_link(&peer(), port, None, queue, |state| { tx.send(state).unwrap(); }));
        (link, worker, rx)
    }

    fn accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let mut socket = None;
        until(|| match listener.accept() {
            Ok((s, _)) => { socket = Some(s); true }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
            Err(e) => panic!("accept: {e}"),
        });
        let socket = socket.unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        socket
    }

    #[test]
    fn queue_keeps_newest_three_seconds() {
        let link = Link::new();
        for n in 0u16..200 {
            link.push(n.to_le_bytes().to_vec());
        }
        let q = link.queue.0.lock().unwrap();
        assert!(q.open);
        assert_eq!(q.chunks.len(), 150);
        assert_eq!(q.dropped, 50);
        let sequence: Vec<_> = q.chunks.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(sequence, (50..200).collect::<Vec<_>>());
    }

    #[test]
    fn reconnects_with_a_fresh_header_and_continuing_pcm() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let (link, worker, status) = sender(listener.local_addr().unwrap().port());
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        link.push(1u16.to_le_bytes().to_vec());
        let mut pcm = [0; 2];
        socket.read_exact(&mut pcm).unwrap();
        assert_eq!(u16::from_le_bytes(pcm), 1);
        let linger = libc::linger { l_onoff: 1, l_linger: 0 };
        assert_eq!(unsafe { libc::setsockopt(socket.as_raw_fd(), libc::SOL_SOCKET, libc::SO_LINGER, &linger as *const _ as *const _, std::mem::size_of_val(&linger) as _) }, 0);
        drop(socket);
        let mut n = 1u16;
        until(|| {
            n += 1;
            link.push(n.to_le_bytes().to_vec());
            match status.try_recv() {
                Ok(state) => { assert_eq!(state, "connecting"); true }
                Err(_) => false,
            }
        });
        let expected: Vec<_> = (n + 1..n + 5).collect();
        for n in &expected { link.push(n.to_le_bytes().to_vec()); }
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        drop(link);
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).unwrap();
        worker.join().unwrap();
        let received: Vec<_> = bytes.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        assert!(received.ends_with(&expected));
        assert!(received.iter().all(|&n| n > 1));
        assert!(received.windows(2).all(|w| w[1] == w[0] + 1));
    }

    #[test]
    fn unreachable_peer_recovers_during_the_same_press() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (link, worker, status) = sender(addr.port());
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "failed");
        let listener = TcpListener::bind(addr).unwrap();
        let started = Instant::now();
        assert_eq!(status.recv_timeout(RETRY + Duration::from_millis(500)).unwrap(), "live");
        assert!(started.elapsed() < RETRY + Duration::from_millis(500));
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        drop(link);
        worker.join().unwrap();
    }

    #[test]
    fn release_drains_all_queued_chunks() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let (link, worker, status) = sender(listener.local_addr().unwrap().port());
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        let expected: Vec<_> = (0..150u16).flat_map(|n| n.to_le_bytes().repeat(FRAME)).collect();
        {
            let mut q = link.queue.0.lock().unwrap();
            for chunk in expected.chunks(FRAME * 2) { q.chunks.push_back(chunk.to_vec()); }
        }
        drop(link);
        let mut received = Vec::new();
        socket.read_to_end(&mut received).unwrap();
        worker.join().unwrap();
        assert_eq!(received, expected);
    }

    #[test]
    fn a_two_and_a_half_second_stall_loses_nothing_and_stays_live() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let size: libc::c_int = 4096;
        assert_eq!(unsafe { libc::setsockopt(listener.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVBUF, &size as *const _ as *const _, std::mem::size_of_val(&size) as _) }, 0);
        let (link, worker, status) = sender(listener.local_addr().unwrap().port());
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        // Far more than real PCM, so the stall outlasts the sender's 4 MB kernel buffer and its writes block.
        let (chunks, per_chunk) = (200u32, 64 * 1024u32);
        let feeder = thread::spawn(move || {
            for n in 0..chunks {
                link.push((0..per_chunk).flat_map(|i| ((n * per_chunk + i) as u16).to_le_bytes()).collect());
                thread::sleep(TICK);
            }
            link
        });
        let mut bytes = vec![0; 640 * 25];
        socket.read_exact(&mut bytes).unwrap();
        thread::sleep(Duration::from_millis(2500));
        let release = thread::spawn(move || drop(feeder.join().unwrap()));
        socket.read_to_end(&mut bytes).unwrap();
        release.join().unwrap();
        worker.join().unwrap();
        assert_eq!(status.try_iter().collect::<Vec<_>>(), Vec::<&str>::new());
        let samples: Vec<_> = bytes.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        assert_eq!(samples.len(), (chunks * per_chunk) as usize);
        assert!(samples.iter().enumerate().all(|(i, &s)| s == i as u16));
    }

    #[test]
    fn release_bounds_an_already_blocked_write() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let size: libc::c_int = 4096;
        assert_eq!(unsafe { libc::setsockopt(listener.as_raw_fd(), libc::SOL_SOCKET, libc::SO_RCVBUF, &size as *const _ as *const _, std::mem::size_of_val(&size) as _) }, 0);
        let (link, worker, status) = sender(listener.local_addr().unwrap().port());
        let mut socket = accept(&listener);
        read_header(&mut socket, None).unwrap();
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        link.push(vec![1; 16 * 1024 * 1024]);
        until(|| link.queue.0.lock().unwrap().chunks.is_empty());
        thread::sleep(Duration::from_millis(100));
        assert!(!worker.is_finished());
        let started = Instant::now();
        drop(link);
        until(|| worker.is_finished());
        worker.join().unwrap();
        assert!(started.elapsed() < DRAIN_TIMEOUT + Duration::from_millis(500));
    }

    #[test]
    fn release_interrupts_retry_without_reconnecting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let (link, worker, status) = sender(addr.port());
        assert_eq!(status.recv_timeout(Duration::from_secs(3)).unwrap(), "failed");
        let started = Instant::now();
        drop(link);
        worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(status.try_recv().is_err());
    }

    #[test]
    fn failed_status_can_recover_but_old_presses_cannot_change_it() {
        let (i, addr, worker, _) = receiver(vec![peer()]);
        i.shared.update(|s| {
            s.intercom.talking = true;
            s.intercom.generation = 2;
            s.intercom.links = vec![("Living Room".into(), "failed")];
        });
        for state in ["connecting", "live", "failed", "live"] {
            link_status(&i.shared, 2, 0, state);
            assert_eq!(i.shared.read(|s| s.intercom.links[0].1), state);
        }
        link_status(&i.shared, 1, 0, "failed");
        assert_eq!(i.shared.read(|s| s.intercom.links[0].1), "live");
        drop(TcpStream::connect(addr).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn real_sender_preserves_pcm_across_odd_writes_and_closes_stream() {
        let (i, addr, worker, _) = receiver(vec![peer()]);
        i.shared.update(|s| s.phase = "speaking");
        let link = Link::new();
        let queue = link.queue.clone();
        let (status_tx, status_rx) = mpsc::channel();
        let sender = thread::spawn(move || send_link(&peer(), addr.port(), None, queue, |s| { status_tx.send(s).unwrap(); }));
        assert_eq!(status_rx.recv_timeout(Duration::from_secs(3)).unwrap(), "live");
        until(|| !i.mixer.lock().unwrap().present().is_empty());
        let pcm = [1234i16, -2345, 32767, -32768];
        let bytes: Vec<_> = pcm.iter().flat_map(|n| n.to_le_bytes()).collect();
        link.push(bytes[..1].to_vec());
        thread::sleep(Duration::from_millis(30));
        assert!(i.mixer.lock().unwrap().streams[&1].samples.is_empty());
        link.push(bytes[1..4].to_vec());
        until(|| i.mixer.lock().unwrap().streams[&1].samples.len() == 2);
        link.push(bytes[4..7].to_vec());
        until(|| i.mixer.lock().unwrap().streams[&1].samples.len() == 3);
        link.push(bytes[7..].to_vec());
        drop(link);
        sender.join().unwrap();
        worker.join().unwrap();
        let mut m = i.mixer.lock().unwrap();
        assert!(!m.streams[&1].open);
        assert_eq!(m.streams[&1].name, "Living Room");
        assert_eq!(&m.frame().unwrap()[..4], &pcm);
        assert!(m.present().is_empty());
        assert_eq!(i.shared.read(|s| s.phase), "speaking");
    }

    #[test]
    fn established_stream_survives_a_sender_pause() {
        let (i, addr, worker, _) = receiver(vec![peer()]);
        let mut socket = TcpStream::connect(addr).unwrap();
        socket.write_all(HEADER).unwrap();
        socket.write_all(&7i16.to_le_bytes()).unwrap();
        until(|| i.mixer.lock().unwrap().streams.get(&1).is_some_and(|s| s.received == 1));
        thread::sleep(Duration::from_millis(2200));
        socket.write_all(&8i16.to_le_bytes()).unwrap();
        drop(socket);
        worker.join().unwrap();
        let mut m = i.mixer.lock().unwrap();
        assert_eq!(m.streams[&1].received, 2);
        assert_eq!(&m.frame().unwrap()[..2], &[7, 8]);
    }

    #[test]
    fn incoming_wakes_screen_and_dismisses_listening_only() {
        for phase in ["listening", "thinking", "speaking"] {
            let (i, addr, worker, rx) = receiver(vec![peer()]);
            i.shared.update(|s| { s.phase = phase; s.screen = false; });
            let mut socket = TcpStream::connect(addr).unwrap();
            socket.write_all(HEADER).unwrap();
            drop(socket);
            worker.join().unwrap();
            assert!(i.shared.read(|s| s.screen));
            assert_eq!(matches!(rx.try_recv(), Ok(Cmd::Dismiss)), phase == "listening");
        }
    }
}
