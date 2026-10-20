//! Doorbell: a ring or a motion detection lights the screen and shows the doorbell camera's
//! snapshots until things go quiet; a ring also plays a chime. A screen it lit goes dark again
//! when the view closes, unless someone touched it meanwhile.
use crate::{audio::Playback, log, screen, Config, Shared, State};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CHIME: &str = "/opt/open-cgs2/ui/sounds/doorbell.wav";
const RING_SHOW: Duration = Duration::from_secs(60);
/// How long the view stays after the last motion sensor turned off.
const MOTION_SHOW: Duration = Duration::from_secs(20);
/// Motion does not reopen the view this soon after it was closed; a ring always does.
const MUTE: Duration = Duration::from_secs(120);
/// When the doorbell lit a dark screen: the screen's last activity then, while nothing has followed.
static WOKE: Mutex<Option<Instant>> = Mutex::new(None);

pub struct Sensor {
    pub entity: String,
    pub label: String,
}

pub struct Doorbell {
    pub name: String,
    pub camera: String,
    /// host:port of the relay that shares snapshots between screens; HA is the fallback.
    pub relay: Option<String>,
    pub ring: Option<String>,
    /// Motion sensors, most specific first: the view names the first one that fired.
    pub motion: Vec<Sensor>,
}

impl Doorbell {
    pub fn from_json(v: &Value) -> Option<Doorbell> {
        Some(Doorbell {
            name: v["name"].as_str().unwrap_or("Doorbell").into(),
            camera: v["camera"].as_str()?.into(),
            relay: v["relay"].as_str().map(String::from),
            ring: v["ring"].as_str().map(String::from),
            motion: v["motion"].as_array().map_or_else(Vec::new, |a| {
                a.iter().filter_map(|m| Some(Sensor { entity: m["entity"].as_str()?.into(), label: m["label"].as_str().unwrap_or("Motion").into() })).collect()
            }),
        })
    }

    pub fn entity_ids(&self) -> Vec<String> {
        let mut ids = vec![self.camera.clone()];
        ids.extend(self.ring.clone());
        ids.extend(self.motion.iter().map(|m| m.entity.clone()));
        ids
    }

    /// 0 for the ring, then the motion sensors in order.
    fn rank(&self, id: &str) -> Option<usize> {
        if self.ring.as_deref() == Some(id) {
            return Some(0);
        }
        self.motion.iter().position(|m| m.entity == id).map(|i| i + 1)
    }

    fn active(&self, s: &State) -> bool {
        self.ring.iter().chain(self.motion.iter().map(|m| &m.entity)).any(|e| s.entities.get(e).map(String::as_str) == Some("on"))
    }
}

struct Event {
    rank: usize,
    label: String,
    /// When it started, or when the doorbell rang, in milliseconds since the epoch.
    at: u64,
    until: Instant,
}

#[derive(Default)]
pub struct Status {
    event: Option<Event>,
    muted: Option<Instant>,
}

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Opens or extends the event for a sensor of `rank` turning on; false if it stays closed.
fn apply(st: &mut Status, rank: usize, label: &str, now: Instant, wall: u64) -> bool {
    let ring = rank == 0;
    if ring {
        st.muted = None;
    } else if st.event.is_none() && st.muted.is_some_and(|t| t > now) {
        return false;
    }
    let until = now + if ring { RING_SHOW } else { MOTION_SHOW };
    match &mut st.event {
        Some(e) => {
            e.until = e.until.max(until);
            if rank < e.rank {
                e.rank = rank;
                e.label = label.into();
                if ring {
                    e.at = wall;
                }
            }
        }
        None => st.event = Some(Event { rank, label: label.into(), at: wall, until }),
    }
    true
}

/// Called when one of the doorbell's sensors turns on.
pub fn fire(shared: &Shared, id: &str) {
    let Some(d) = &shared.cfg.doorbell else { return };
    let Some(rank) = d.rank(id) else { return };
    let label = if rank == 0 { "Ring" } else { &d.motion[rank - 1].label };
    let dark = screen::lit() == Some(false);
    if rank > 0 && dark && screen::local_time().is_some_and(|(m, _)| m < screen::NIGHT_END) {
        log(format!("doorbell: {label} at night, screen left off"));
        return;
    }
    let mut shown = false;
    shared.update(|s| shown = apply(&mut s.doorbell, rank, label, Instant::now(), epoch_ms()));
    if !shown {
        return;
    }
    log(format!("doorbell: {label}"));
    if rank == 0 {
        chime();
    }
    let mut woke = WOKE.lock().unwrap();
    let untouched = woke.is_some() && *woke == screen::last_active();
    screen::set(shared, true, "doorbell");
    *woke = if dark || untouched { screen::last_active() } else { None };
}

pub fn dismiss(shared: &Shared) {
    shared.update(|s| {
        if s.doorbell.event.take().is_some() {
            s.doorbell.muted = Some(Instant::now() + MUTE);
        }
    });
    CHIMING.lock().unwrap().take();
}

pub fn snapshot(cfg: &Config, s: &State) -> Value {
    let (Some(d), Some(e)) = (&cfg.doorbell, &s.doorbell.event) else { return Value::Null };
    let path = s.attrs.get(&d.camera).and_then(|a| a.get("entity_picture")?.as_str());
    let picture = path.map(|p| format!("http://{}{p}", cfg.ha));
    let relay = path.zip(d.relay.as_ref()).map(|(p, r)| format!("http://{r}{p}"));
    json!({"name": d.name, "ring": e.rank == 0, "label": e.label, "at": e.at, "picture": picture, "relay": relay})
}

/// Closes the view once the ring's minute is up and no sensor has been on for a while.
pub fn run(shared: Arc<Shared>) {
    let Some(d) = &shared.cfg.doorbell else { return };
    let mut quiet_since = Instant::now();
    loop {
        thread::sleep(Duration::from_millis(250));
        let now = Instant::now();
        let (open, active, until) = shared.read(|s| (s.doorbell.event.is_some(), d.active(s), s.doorbell.event.as_ref().map(|e| e.until)));
        if active || !open {
            quiet_since = now;
        }
        if !open {
            let mut woke = WOKE.lock().unwrap();
            if woke.take().is_some_and(|w| screen::last_active() == Some(w)) {
                screen::set(&shared, false, "doorbell over");
            }
        }
        if open && !active && until.is_some_and(|t| t <= now) && now - quiet_since >= MOTION_SHOW {
            shared.update(|s| s.doorbell.event = None);
        }
    }
}

static CHIMING: Mutex<Option<Playback>> = Mutex::new(None);

fn chime() {
    let mut c = CHIMING.lock().unwrap();
    c.take();
    let args = ["-q", "filesrc", &format!("location={CHIME}"), "!", "wavparse", "!", "audioconvert", "!", "audioresample", "!", "alsasink"].map(String::from).to_vec();
    *c = Some(Playback::new(args, false, || {}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_outrank_motion_and_get_past_a_dismissal() {
        let (mut st, now) = (Status::default(), Instant::now());
        assert!(apply(&mut st, 2, "Motion", now, 1));
        assert!(apply(&mut st, 1, "Person", now, 2));
        assert!(apply(&mut st, 2, "Motion", now, 3));
        let e = st.event.as_ref().unwrap();
        assert_eq!((e.rank, e.label.as_str(), e.at), (1, "Person", 1));
        assert!(apply(&mut st, 0, "Ring", now, 4));
        let e = st.event.as_ref().unwrap();
        assert_eq!((e.rank, e.at, e.until), (0, 4, now + RING_SHOW));

        st = Status { event: None, muted: Some(now + MUTE) };
        assert!(!apply(&mut st, 1, "Person", now, 5));
        assert!(apply(&mut st, 1, "Person", now + MUTE, 6));
        st = Status { event: None, muted: Some(now + MUTE) };
        assert!(apply(&mut st, 0, "Ring", now, 7));
        assert!(st.muted.is_none());
    }
}
