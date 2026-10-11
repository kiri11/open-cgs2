//! Timers and alarms for the clock screen. Alarms are kept in clock.json; timers last until the
//! backend restarts. One going off lights the screen and loops a stock ringtone until it is
//! stopped or snoozed, for at most 10 minutes.
use crate::{audio::Playback, log, screen, Shared};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STORE: &str = "/userdata/open-cgs2/clock.json";
const TONES: &str = "/qingping/data/resources";
const TIMER_TONE: &str = "0961bb77";
const ALARM_TONE: &str = "6e70b659";
const RING_FOR: Duration = Duration::from_secs(600);
const SNOOZE: Duration = Duration::from_secs(540);
const LONGEST_TIMER: u64 = 7 * 86400;

#[derive(Clone, Debug, PartialEq)]
pub struct Alarm {
    pub id: u64,
    /// Minutes after local midnight.
    pub time: u32,
    /// Weekdays it repeats on, 0 = Sunday; with none it rings once.
    pub days: Vec<u32>,
    pub on: bool,
    pub tone: String,
}

impl Alarm {
    fn from_json(v: &Value) -> Option<Alarm> {
        let mut days: Vec<u32> = v["days"].as_array().map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_u64).filter(|d| *d < 7).map(|d| d as u32).collect());
        days.sort_unstable();
        days.dedup();
        Some(Alarm {
            id: v["id"].as_u64().unwrap_or(0),
            time: screen::parse_time(v["time"].as_str()?)?,
            days,
            on: v["on"].as_bool().unwrap_or(true),
            tone: v["tone"].as_str().filter(|t| valid_tone(t)).unwrap_or(ALARM_TONE).into(),
        })
    }

    fn json(&self) -> Value {
        json!({"id": self.id, "time": hhmm(self.time), "days": self.days, "on": self.on, "tone": self.tone})
    }

    /// Whether it rings at a minute in (prev, now], `wday` being the weekday at `now`.
    fn due(&self, prev: u32, now: u32, wday: u32) -> bool {
        let day = if self.time <= now { wday } else { (wday + 6) % 7 };
        self.on && screen::passed(prev, now, self.time) && (self.days.is_empty() || self.days.contains(&day))
    }
}

enum Run {
    Until(Instant),
    Paused(Duration),
}

struct Timer {
    id: u64,
    total: u64,
    run: Run,
}

struct Ringing {
    kind: &'static str,
    id: u64,
    label: String,
    since: Instant,
}

#[derive(Default)]
pub struct Clock {
    alarms: Vec<Alarm>,
    timers: Vec<Timer>,
    snoozed: Option<(u64, Instant)>,
    ringing: Option<Ringing>,
    next_id: u64,
}

impl Clock {
    fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn save(&self) {
        let v = json!({"alarms": self.alarms.iter().map(Alarm::json).collect::<Vec<_>>()});
        if let Err(e) = std::fs::write(STORE, v.to_string()) {
            log(format!("{STORE}: {e}"));
        }
    }

    pub fn ringing(&self) -> bool {
        self.ringing.is_some()
    }
}

pub fn load() -> Clock {
    let v: Value = std::fs::read_to_string(STORE).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut alarms: Vec<Alarm> = v["alarms"].as_array().map_or_else(Vec::new, |a| a.iter().filter_map(Alarm::from_json).filter(|a| a.id > 0).collect());
    alarms.sort_by_key(|a| a.time);
    let next_id = alarms.iter().map(|a| a.id).max().unwrap_or(0);
    Clock { alarms, next_id, ..Clock::default() }
}

fn valid_tone(t: &str) -> bool {
    t.len() == 8 && t.bytes().all(|b| b.is_ascii_hexdigit())
}

fn hhmm(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn span(secs: u64) -> String {
    let parts = [(secs / 3600, "h"), (secs % 3600 / 60, "min"), (secs % 60, "s")];
    parts.iter().filter(|(n, _)| *n > 0).map(|(n, u)| format!("{n} {u}")).collect::<Vec<_>>().join(" ")
}

fn epoch_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub fn snapshot(c: &Clock) -> Value {
    let (now, wall) = (Instant::now(), epoch_ms());
    let at = |i: Instant| wall + i.saturating_duration_since(now).as_millis() as u64;
    let timers: Vec<Value> = c
        .timers
        .iter()
        .map(|t| match t.run {
            Run::Until(end) => json!({"id": t.id, "total": t.total, "running": true, "end": at(end)}),
            Run::Paused(left) => json!({"id": t.id, "total": t.total, "running": false, "left": left.as_millis() as u64}),
        })
        .collect();
    json!({
        "alarms": c.alarms.iter().map(Alarm::json).collect::<Vec<_>>(),
        "timers": timers,
        "ringing": c.ringing.as_ref().map(|r| json!({"kind": r.kind, "id": r.id, "label": r.label})),
        "snoozed": c.snoozed.map(|(_, until)| at(until)),
    })
}

/// The stock ringtones, from the Qingping app's list.
pub fn tones() -> Value {
    let v: Value = std::fs::read_to_string(format!("{TONES}/ringtones.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let list = v["data"].as_array().map_or_else(Vec::new, |a| a.iter().filter_map(|t| Some(json!({"key": t["key"].as_str()?, "name": t["name_en"].as_str()?}))).collect());
    Value::Array(list)
}

/// Handles POST /clock/<path>; saving an alarm takes it as the JSON `body`.
pub fn command(shared: &Shared, path: &str, body: &Value) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["timer", "start", secs] => {
            let Some(secs) = secs.parse().ok().filter(|s| (1..=LONGEST_TIMER).contains(s)) else {
                return false;
            };
            shared.update(|s| {
                let id = s.clock.take_id();
                s.clock.timers.push(Timer { id, total: secs, run: Run::Until(Instant::now() + Duration::from_secs(secs)) });
            });
            true
        }
        ["timer", id, action @ ("pause" | "resume" | "cancel")] => {
            let Ok(id) = id.parse::<u64>() else {
                return false;
            };
            let mut found = false;
            shared.update(|s| {
                let timers = &mut s.clock.timers;
                let Some(i) = timers.iter().position(|t| t.id == id) else {
                    return;
                };
                found = true;
                let now = Instant::now();
                match (*action, &timers[i].run) {
                    ("cancel", _) => drop(timers.remove(i)),
                    ("pause", Run::Until(end)) => timers[i].run = Run::Paused(end.saturating_duration_since(now)),
                    ("resume", Run::Paused(left)) => timers[i].run = Run::Until(now + *left),
                    _ => {}
                }
            });
            found
        }
        ["alarm"] => {
            let Some(mut alarm) = Alarm::from_json(body) else {
                return false;
            };
            alarm.on = true;
            shared.update(|s| {
                let c = &mut s.clock;
                match c.alarms.iter_mut().find(|a| alarm.id != 0 && a.id == alarm.id) {
                    Some(a) => *a = alarm,
                    None => {
                        alarm.id = c.take_id();
                        c.alarms.push(alarm);
                    }
                }
                c.alarms.sort_by_key(|a| a.time);
                c.save();
            });
            true
        }
        ["alarm", id, action @ ("on" | "off" | "delete")] => {
            let Ok(id) = id.parse::<u64>() else {
                return false;
            };
            let mut found = false;
            shared.update(|s| {
                let c = &mut s.clock;
                let Some(i) = c.alarms.iter().position(|a| a.id == id) else {
                    return;
                };
                found = true;
                if *action == "delete" {
                    c.alarms.remove(i);
                } else {
                    c.alarms[i].on = *action == "on";
                }
                if *action != "on" && c.snoozed.is_some_and(|(s, _)| s == id) {
                    c.snoozed = None;
                }
                c.save();
            });
            found
        }
        ["stop"] => {
            stop(shared);
            true
        }
        ["snooze"] => {
            shared.update(|s| {
                if let Some(r) = s.clock.ringing.take() {
                    if r.kind == "alarm" {
                        s.clock.snoozed = Some((r.id, Instant::now() + SNOOZE));
                    }
                }
            });
            silence();
            true
        }
        ["snooze", "cancel"] => {
            shared.update(|s| s.clock.snoozed = None);
            true
        }
        ["preview", "stop"] => {
            if !shared.read(|s| s.clock.ringing()) {
                silence();
            }
            true
        }
        ["preview", tone] if valid_tone(tone) => {
            if !shared.read(|s| s.clock.ringing()) {
                play(tone, false);
            }
            true
        }
        _ => false,
    }
}

fn ring(shared: &Shared, kind: &'static str, id: u64, label: String, tone: &str) {
    log(format!("{kind} {label} going off"));
    shared.update(|s| s.clock.ringing = Some(Ringing { kind, id, label, since: Instant::now() }));
    play(tone, true);
    screen::set(shared, true, kind);
}

/// Silences whatever is ringing; false if nothing was.
pub fn stop(shared: &Shared) -> bool {
    if !shared.read(|s| s.clock.ringing()) {
        return false;
    }
    shared.update(|s| s.clock.ringing = None);
    silence();
    true
}

pub fn run(shared: Arc<Shared>) {
    let mut prev = screen::local_time();
    loop {
        thread::sleep(Duration::from_millis(250));
        let now = Instant::now();
        let finished: Vec<(u64, u64)> = shared.read(|s| {
            s.clock.timers.iter().filter(|t| matches!(t.run, Run::Until(end) if end <= now)).map(|t| (t.id, t.total)).collect()
        });
        if let Some(&(id, total)) = finished.first() {
            shared.update(|s| s.clock.timers.retain(|t| !finished.iter().any(|(f, _)| *f == t.id)));
            ring(&shared, "timer", id, span(total), TIMER_TONE);
        }

        let local = screen::local_time();
        if let (Some((p, _)), Some((m, wday))) = (prev, local) {
            let due: Vec<Alarm> = shared.read(|s| s.clock.alarms.iter().filter(|a| a.due(p, m, wday)).cloned().collect());
            if let Some(a) = due.first() {
                if due.iter().any(|a| a.days.is_empty()) {
                    shared.update(|s| {
                        s.clock.alarms.iter_mut().filter(|a| a.days.is_empty() && due.iter().any(|d| d.id == a.id)).for_each(|a| a.on = false);
                        s.clock.save();
                    });
                }
                ring(&shared, "alarm", a.id, hhmm(a.time), &a.tone);
            }
        }
        prev = local;

        let snoozed = shared.read(|s| s.clock.snoozed.filter(|(_, at)| *at <= now).map(|(id, _)| id));
        if let Some(id) = snoozed {
            shared.update(|s| s.clock.snoozed = None);
            if let Some(a) = shared.read(|s| s.clock.alarms.iter().find(|a| a.id == id).cloned()) {
                ring(&shared, "alarm", a.id, hhmm(a.time), &a.tone);
            }
        }

        if shared.read(|s| s.clock.ringing.as_ref().is_some_and(|r| r.since.elapsed() >= RING_FOR)) {
            log("stopped ringing after 10 minutes");
            stop(&shared);
        }
    }
}

static SOUND: Mutex<Option<Playback>> = Mutex::new(None);

fn play(tone: &str, repeat: bool) {
    let mut s = SOUND.lock().unwrap();
    s.take();
    let args = ["-q", "filesrc", &format!("location={TONES}/{tone}.wav"), "!", "wavparse", "!", "audioconvert", "!", "audioresample", "!", "alsasink"].map(String::from).to_vec();
    *s = Some(Playback::new(args, repeat, || {}));
}

fn silence() {
    SOUND.lock().unwrap().take();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarms_ring_on_their_days() {
        let alarm = |time, days: &[u32]| Alarm { id: 1, time, days: days.to_vec(), on: true, tone: ALARM_TONE.into() };
        let weekdays = alarm(450, &[1, 2, 3, 4, 5]);
        assert!(weekdays.due(449, 450, 1));
        assert!(!weekdays.due(449, 450, 0));
        assert!(!weekdays.due(450, 451, 1));
        let midnight = alarm(0, &[6]);
        assert!(midnight.due(1439, 0, 6));
        assert!(!midnight.due(1439, 0, 5));
        let late = alarm(1439, &[5]);
        assert!(late.due(1438, 1, 6));
        assert!(alarm(450, &[]).due(449, 450, 3));
    }
}
