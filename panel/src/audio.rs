use crate::{ha::Cmd, log};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct MicConfig {
    pub device: String,
    pub channels: usize,
    pub channel: usize,
    pub gain: f32,
}

/// 20 ms at 16 kHz.
const CHUNK_FRAMES: usize = 320;
/// The PDM microphones click for a few tens of milliseconds after opening.
const SKIP_CHUNKS: usize = 3;

struct Capture {
    on: bool,
    generation: u64,
    pid: u32,
}

pub struct Mic {
    active: Arc<(Mutex<Capture>, Condvar)>,
}

impl Mic {
    pub fn new(cfg: MicConfig, sink: impl Fn(Vec<u8>) -> bool + Send + 'static) -> Mic {
        let active = Arc::new((Mutex::new(Capture { on: false, generation: 0, pid: 0 }), Condvar::new()));
        let a = active.clone();
        thread::spawn(move || capture(cfg, a, sink));
        Mic { active }
    }

    pub fn start(&self) {
        let (m, c) = &*self.active;
        let mut a = m.lock().unwrap();
        if !a.on {
            a.generation += 1;
            a.on = true;
            c.notify_all();
        }
    }

    pub fn stop(&self) {
        let mut a = self.active.0.lock().unwrap();
        a.on = false;
        a.generation += 1;
        kill(a.pid);
    }
}

fn capture(cfg: MicConfig, active: Arc<(Mutex<Capture>, Condvar)>, sink: impl Fn(Vec<u8>) -> bool) {
    let (m, c) = &*active;
    let channels = cfg.channels.to_string();
    loop {
        let mut a = c.wait_while(m.lock().unwrap(), |a| !a.on).unwrap();
        let generation = a.generation;
        let spawned = Command::new("arecord")
            .args(["-q", "-D", &cfg.device, "-f", "S16_LE", "-r", "16000", "-c", &channels, "-t", "raw"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                log(format!("arecord: {e}"));
                a.on = false;
                continue;
            }
        };
        a.pid = child.id();
        drop(a);
        let is_active = || { let a = m.lock().unwrap(); a.on && a.generation == generation };
        let mut out = child.stdout.take().unwrap();
        let mut buf = vec![0u8; CHUNK_FRAMES * cfg.channels * 2];
        let mut n = 0;
        let mut keep = true;
        while is_active() && out.read_exact(&mut buf).is_ok() && is_active() {
            n += 1;
            if n <= SKIP_CHUNKS {
                continue;
            }
            let mut pcm = Vec::with_capacity(CHUNK_FRAMES * 2);
            for frame in buf.chunks_exact(cfg.channels * 2) {
                let i = cfg.channel * 2;
                let s = i16::from_le_bytes([frame[i], frame[i + 1]]) as f32 * cfg.gain;
                pcm.extend_from_slice(&(s.clamp(-32768.0, 32767.0) as i16).to_le_bytes());
            }
            if !sink(pcm) {
                keep = false;
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let mut a = m.lock().unwrap();
        a.pid = 0;
        if !keep {
            return;
        }
        if a.on && a.generation == generation {
            log("arecord stopped unexpectedly");
            a.on = false;
        }
    }
}

struct Gate {
    holds: usize,
    generation: u64,
    pids: Vec<u32>,
}

static GATE: Mutex<Gate> = Mutex::new(Gate { holds: 0, generation: 0, pids: Vec::new() });
static FREE: Condvar = Condvar::new();

/// Keeps ordinary playback silent until the holder has closed its speaker.
pub struct SpeakerHold;

pub fn hold_speaker() -> SpeakerHold {
    let mut g = GATE.lock().unwrap();
    g.holds += 1;
    if g.holds == 1 {
        g.generation += 1;
        for &pid in &g.pids {
            kill(pid);
        }
        let until = Instant::now() + Duration::from_millis(500);
        while g.pids.iter().any(|&pid| running(pid)) && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
    }
    SpeakerHold
}

impl Drop for SpeakerHold {
    fn drop(&mut self) {
        let mut g = GATE.lock().unwrap();
        g.holds -= 1;
        FREE.notify_all();
    }
}

fn kill(pid: u32) {
    if pid != 0 {
        unsafe { libc::kill(pid as i32, libc::SIGKILL); }
    }
}

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().is_some_and(|s| {
        !matches!(s.rsplit_once(") ").and_then(|(_, tail)| tail.chars().next()), Some('Z' | 'X'))
    })
}

/// Restarts interrupted playback from the beginning when the speaker becomes free.
pub struct Playback {
    stopped: Arc<AtomicBool>,
    pid: Arc<AtomicU32>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Playback {
    pub fn new(args: Vec<String>, repeat: bool, done: impl FnOnce() + Send + 'static) -> Playback {
        Self::with_program("gst-launch-1.0".into(), args, repeat, done)
    }

    fn with_program(program: String, args: Vec<String>, repeat: bool, done: impl FnOnce() + Send + 'static) -> Playback {
        let stopped = Arc::new(AtomicBool::new(false));
        let pid = Arc::new(AtomicU32::new(0));
        let (cancel, current) = (stopped.clone(), pid.clone());
        let worker = thread::spawn(move || {
            loop {
                let mut g = FREE.wait_while(GATE.lock().unwrap(), |g| g.holds > 0 && !cancel.load(Ordering::SeqCst)).unwrap();
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                let started = Instant::now();
                let spawned = Command::new(&program).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
                let mut child = match spawned {
                    Ok(c) => c,
                    Err(e) => {
                        log(format!("{program}: {e}"));
                        drop(g);
                        break;
                    }
                };
                let generation = g.generation;
                let id = child.id();
                current.store(id, Ordering::SeqCst);
                g.pids.push(id);
                drop(g);
                let _ = child.wait();
                let mut g = GATE.lock().unwrap();
                current.store(0, Ordering::SeqCst);
                g.pids.retain(|&p| p != id);
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                if generation != g.generation {
                    continue;
                }
                if !repeat {
                    break;
                }
                if started.elapsed() < Duration::from_secs(1) {
                    let _ = FREE.wait_timeout_while(g, Duration::from_secs(1), |_| !cancel.load(Ordering::SeqCst)).unwrap();
                }
            }
            if !cancel.load(Ordering::SeqCst) {
                done();
            }
        });
        Playback { stopped, pid, worker: Mutex::new(Some(worker)) }
    }

    pub fn stop(&self) {
        {
            let _g = GATE.lock().unwrap();
            self.stopped.store(true, Ordering::SeqCst);
            kill(self.pid.load(Ordering::SeqCst));
            FREE.notify_all();
        }
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Streams TTS audio from a URL to the speaker with GStreamer.
pub struct Player {
    current: Mutex<Option<Playback>>,
    tx: Sender<Cmd>,
}

impl Player {
    pub fn new(tx: Sender<Cmd>) -> Player {
        Player { current: Mutex::new(None), tx }
    }

    /// Plays `url`; sends `Cmd::PlayerDone(token)` when playback ends on its own.
    pub fn play(&self, url: &str, token: u64) {
        let mut current = self.current.lock().unwrap();
        current.take();
        let args = ["-q", "souphttpsrc", &format!("location={url}"), "!", "decodebin", "!", "audioconvert", "!", "audioresample", "!", "alsasink"].map(String::from).to_vec();
        let tx = self.tx.clone();
        *current = Some(Playback::new(args, false, move || { let _ = tx.send(Cmd::PlayerDone(token)); }));
    }

    pub fn stop(&self) {
        self.current.lock().unwrap().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn until(mut f: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_secs(2);
        while !f() {
            assert!(Instant::now() < end, "timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn holds_kill_and_replay_without_spawning_while_held() {
        let _serial = SERIAL.lock().unwrap();
        let path = std::env::temp_dir().join(format!("cgs2-playback-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let command = format!("echo run >> '{}'; exec sleep 5", path.display());
        let (tx, rx) = mpsc::channel();
        let p = Playback::with_program("sh".into(), vec!["-c".into(), command], false, move || { tx.send(()).unwrap(); });
        let runs = || std::fs::read_to_string(&path).unwrap_or_default().lines().count();
        until(|| runs() == 1);
        let pid = p.pid.load(Ordering::SeqCst);
        let at = Instant::now();
        let hold = hold_speaker();
        assert!(at.elapsed() < Duration::from_millis(500));
        assert!(!running(pid));
        let second = hold_speaker();
        drop(hold);
        thread::sleep(Duration::from_millis(100));
        assert_eq!(runs(), 1);
        assert!(rx.try_recv().is_err());
        drop(second);
        until(|| runs() == 2);
        p.stop();
        assert!(rx.try_recv().is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn completion_runs_once_and_stop_suppresses_it() {
        let _serial = SERIAL.lock().unwrap();
        let (tx, rx) = mpsc::channel();
        let p = Playback::with_program("sh".into(), vec!["-c".into(), "exit 0".into()], false, move || { tx.send(()).unwrap(); });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        p.stop();
        assert!(rx.try_recv().is_err());
        let (tx, rx) = mpsc::channel();
        let hold = hold_speaker();
        let p = Playback::with_program("sleep".into(), vec!["5".into()], false, move || { tx.send(()).unwrap(); });
        p.stop();
        drop(hold);
        assert!(rx.try_recv().is_err());
    }
}
