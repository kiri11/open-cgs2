//! Screen power and brightness, as the stock app handled them. The level follows the room's light
//! (sensord's reading) with the app's curve, smoothing and ramp, or stays at a level set on the
//! Settings screen. On battery the screen goes off after 5 minutes without a touch (also set
//! there), and plugging or unplugging the charger lights it. A screen lit at midnight goes off
//! until 8 am. A short
//! press of the power key switches it, or silences a ringing timer or alarm; holding the key for
//! 1.5 s asks whether to power off.
use crate::{log, Shared};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const BACKLIGHT: &str = "/sys/class/backlight/backlight/brightness";
const POWER_KEY: &str = "/dev/input/event0";
const TOUCH: &str = "/dev/input/event1";
const SENSORS: &str = "/run/open-cgs2/sensors";
const PREFS: &str = "/userdata/open-cgs2/screen.json";
/// Marks a screen turned off at midnight, so that it comes back on when the night ends.
const NIGHT_OFF: &str = "/run/open-cgs2/night-off";
/// Minutes after local midnight when the night ends.
pub const NIGHT_END: u32 = 8 * 60;
const EV_KEY: u16 = 1;
const KEY_POWER: u16 = 116;
const EVENT_SIZE: usize = std::mem::size_of::<libc::timeval>() + 8;
const LONG_PRESS: Duration = Duration::from_millis(1500);
const RAMP_TICK: Duration = Duration::from_millis(30);
const RAMP_STEP: i32 = 3;

/// The last touch or key press.
static ACTIVE: Mutex<Option<Instant>> = Mutex::new(None);
/// The backlight level while on, ramping towards the one for the room's light.
static LEVEL: AtomicU32 = AtomicU32::new(500);
static PREFERENCES: Mutex<Option<Prefs>> = Mutex::new(None);

/// What the Settings screen sets, kept in screen.json.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    /// Follow the room's light; otherwise stay at `level`.
    pub auto: bool,
    /// Backlight level, 20..=1000.
    pub level: u32,
    /// Minutes without a touch before the screen turns off on battery; None keeps it on.
    pub idle: Option<u64>,
}

impl Prefs {
    fn from_json(v: &Value) -> Prefs {
        Prefs {
            auto: v["auto"].as_bool().unwrap_or(true),
            level: v["level"].as_u64().map_or(500, |l| l.clamp(20, 1000) as u32),
            idle: match &v["idle"] {
                Value::Null => Some(5),
                i => i.as_u64().filter(|m| *m > 0),
            },
        }
    }

    pub fn json(&self) -> Value {
        json!({"auto": self.auto, "level": self.level, "idle": self.idle.unwrap_or(0)})
    }
}

pub fn prefs() -> Prefs {
    *PREFERENCES.lock().unwrap().get_or_insert_with(|| {
        Prefs::from_json(&std::fs::read_to_string(PREFS).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
    })
}

/// Takes the Settings screen's choices (`idle` 0 = never) and applies a fixed level at once.
pub fn set_prefs(v: &Value) {
    let p = Prefs::from_json(&json!({"auto": v["auto"], "level": v["level"], "idle": v["idle"].as_u64().unwrap_or(0)}));
    *PREFERENCES.lock().unwrap() = Some(p);
    if let Err(e) = std::fs::write(PREFS, p.json().to_string()) {
        log(format!("{PREFS}: {e}"));
    }
    if !p.auto {
        LEVEL.store(p.level, Ordering::Relaxed);
        if lit() == Some(true) {
            write_backlight(p.level);
        }
    }
}

pub fn parse_time(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn brightness() -> Option<u32> {
    std::fs::read_to_string(BACKLIGHT).ok()?.trim().parse().ok()
}

pub fn lit() -> Option<bool> {
    brightness().map(|b| b > 0)
}

fn write_backlight(level: u32) {
    if let Err(e) = std::fs::write(BACKLIGHT, level.to_string()) {
        log(format!("backlight: {e}"));
    }
}

fn active() {
    *ACTIVE.lock().unwrap() = Some(Instant::now());
}

pub fn last_active() -> Option<Instant> {
    *ACTIVE.lock().unwrap()
}

/// The app's backlight level (20..1000) for the averaged light in lux, piecewise linear.
pub fn level_for(lux: i32) -> i32 {
    const SEGMENTS: [(i32, i32, i32, i32); 4] = [(4, 21, 50, 300), (51, 301, 300, 600), (301, 601, 900, 900), (901, 901, 1500, 1000)];
    if lux <= 3 {
        return 20;
    }
    for (x1, y1, x2, y2) in SEGMENTS {
        if lux <= x2 {
            let slope = ((y1 - y2) as f64 / (x1 - x2) as f64) as f32;
            let icpt = (y1 as f64 - slope as f64 * x1 as f64) as f32;
            return slope.mul_add(lux as f32, icpt) as i32;
        }
    }
    1000
}

/// The app's light filter: the mean of the last 10 readings, skipping any that changed faster
/// than 200 lux/s since the last one kept.
#[derive(Default)]
struct LightFilter {
    kept: std::collections::VecDeque<i32>,
    at: Option<Instant>,
}

impl LightFilter {
    fn update(&mut self, lux: i32, now: Instant) -> i32 {
        let jump = match (self.kept.back(), self.at) {
            (Some(&last), Some(at)) => (lux - last).abs() as f64 / now.duration_since(at).as_secs_f64().max(1.0) > 200.0,
            _ => false,
        };
        if !jump {
            self.kept.push_back(lux);
            self.at = Some(now);
            if self.kept.len() > 10 {
                self.kept.pop_front();
            }
        }
        self.kept.iter().sum::<i32>() / self.kept.len().max(1) as i32
    }
}

pub fn set(shared: &Shared, on: bool, why: &str) {
    active();
    if lit() == Some(on) {
        return;
    }
    log(format!("screen {} ({why})", if on { "on" } else { "off" }));
    write_backlight(if on { LEVEL.load(Ordering::Relaxed) } else { 0 });
    shared.update(|s| {
        s.screen = on;
        s.power_menu &= on;
    });
}

/// Powers off or reboots, as the app's power menu did; sensord saves its state on the way down.
pub fn power(shared: &Shared, action: &str) -> bool {
    let cmd: &[&str] = match action {
        "reboot" => &["reboot"],
        "off" if room().1 == Some(true) => &["reboot", "charge"],
        "off" => &["poweroff"],
        "cancel" => &[],
        _ => return false,
    };
    shared.update(|s| s.power_menu = false);
    if !cmd.is_empty() {
        log(format!("power {action}"));
        if let Err(e) = std::process::Command::new(cmd[0]).args(&cmd[1..]).spawn() {
            log(format!("{}: {e}", cmd[0]));
        }
    }
    true
}

/// Calls `f` with each input event's type, code and value.
fn watch(path: &str, mut f: impl FnMut(u16, u16, i32)) {
    let mut dev = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return log(format!("{path}: {e}")),
    };
    let mut ev = [0u8; EVENT_SIZE];
    while dev.read_exact(&mut ev).is_ok() {
        let at = EVENT_SIZE - 8;
        let kind = u16::from_ne_bytes([ev[at], ev[at + 1]]);
        let code = u16::from_ne_bytes([ev[at + 2], ev[at + 3]]);
        let value = i32::from_ne_bytes([ev[at + 4], ev[at + 5], ev[at + 6], ev[at + 7]]);
        f(kind, code, value);
    }
    log(format!("{path}: reader stopped"));
}

/// Short presses switch the screen; a 1.5 s hold lights it, or opens the power menu if lit.
fn watch_key(shared: Arc<Shared>) {
    let down: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    {
        let (shared, down) = (shared.clone(), down.clone());
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(50));
            let held = down.lock().unwrap().is_some_and(|t| t.elapsed() >= LONG_PRESS);
            if held {
                *down.lock().unwrap() = None;
                if lit() == Some(true) {
                    shared.update(|s| s.power_menu = true);
                } else {
                    set(&shared, true, "power key held");
                }
            }
        });
    }
    watch(POWER_KEY, |kind, code, value| {
        if kind != EV_KEY || code != KEY_POWER {
            return;
        }
        match value {
            1 => *down.lock().unwrap() = Some(Instant::now()),
            0 => {
                if down.lock().unwrap().take().is_some() && !crate::clock::stop(&shared) {
                    if let Some(l) = lit() {
                        set(&shared, !l, "power key");
                    }
                }
            }
            _ => {}
        }
    });
}

/// Minutes after local midnight and the weekday, 0 = Sunday.
pub fn local_time() -> Option<(u32, u32)> {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return None;
        }
        Some((tm.tm_hour as u32 * 60 + tm.tm_min as u32, tm.tm_wday as u32))
    }
}

/// Whether minute `at` lies in (prev, now], across midnight. Larger steps are clock jumps.
pub fn passed(prev: u32, now: u32, at: u32) -> bool {
    let step = (now + 1440 - prev) % 1440;
    let to = (at + 1440 - prev) % 1440;
    step <= 5 && to > 0 && to <= step
}

/// sensord's light reading and whether the battery is charging.
fn room() -> (Option<i32>, Option<bool>) {
    let v: serde_json::Value = std::fs::read_to_string(SENSORS).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    (v["illuminance"].as_i64().map(|l| l as i32), v["charging"].as_bool())
}

pub fn run(shared: Arc<Shared>) {
    {
        let shared = shared.clone();
        thread::spawn(move || watch_key(shared));
    }
    thread::spawn(|| watch(TOUCH, |_, _, _| active()));
    let uptime: f64 = std::fs::read_to_string("/proc/uptime").ok().and_then(|u| u.split_whitespace().next()?.parse().ok()).unwrap_or(0.0);
    if uptime < 180.0 {
        set(&shared, true, "boot");
    }
    active();
    let mut prev = local_time().map(|(m, _)| m);
    let mut filter = LightFilter::default();
    let mut target: Option<i32> = None;
    let mut charging = None;
    let mut next = Instant::now();
    loop {
        thread::sleep(RAMP_TICK);
        if let Some(t) = target {
            let current = LEVEL.load(Ordering::Relaxed) as i32;
            let step = if t > current { RAMP_STEP } else { -RAMP_STEP };
            let level = if (current + step - t).abs() < RAMP_STEP { t } else { current + step };
            LEVEL.store(level as u32, Ordering::Relaxed);
            if level == t {
                target = None;
            }
            if lit() == Some(true) {
                write_backlight(level as u32);
            }
        }
        if Instant::now() < next {
            continue;
        }
        next += Duration::from_secs(1);
        let (light, plugged) = room();
        let p = prefs();
        let current = LEVEL.load(Ordering::Relaxed) as i32;
        if !p.auto {
            target = (p.level as i32 != current).then_some(p.level as i32);
        } else if let Some(lux) = light {
            let want = level_for(filter.update(lux, Instant::now()));
            target = ((want - current).abs() >= 10).then_some(want);
        }
        if plugged.is_some() && charging.is_some() && plugged != charging {
            set(&shared, true, if plugged == Some(true) { "charger plugged in" } else { "charger unplugged" });
        }
        charging = plugged.or(charging);
        if let Some(b) = brightness() {
            if shared.read(|s| s.screen != (b > 0)) {
                shared.update(|s| s.screen = b > 0);
            }
            let idle = ACTIVE.lock().unwrap().map_or(Duration::ZERO, |t| t.elapsed());
            let ringing = shared.read(|s| s.clock.ringing() || s.call.open());
            if b > 0 && charging == Some(false) && !ringing && p.idle.is_some_and(|m| idle >= Duration::from_secs(m * 60)) {
                set(&shared, false, "idle on battery");
            }
        }
        let now = local_time().map(|(m, _)| m);
        if let (Some(p), Some(n)) = (prev, now) {
            if passed(p, n, 0) && lit() == Some(true) {
                set(&shared, false, "night");
                if let Err(e) = std::fs::write(NIGHT_OFF, "") {
                    log(format!("{NIGHT_OFF}: {e}"));
                }
            }
            if passed(p, n, NIGHT_END) && std::fs::remove_file(NIGHT_OFF).is_ok() {
                set(&shared, true, "night over");
            }
        }
        prev = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_apps_brightness_curve() {
        assert_eq!([level_for(0), level_for(4), level_for(50), level_for(51), level_for(300), level_for(1500), level_for(5000)], [20, 21, 300, 301, 600, 1000, 1000]);
        let mut f = LightFilter::default();
        let t = Instant::now();
        assert_eq!(f.update(50, t), 50);
        assert_eq!(f.update(1000, t + Duration::from_secs(1)), 50);
        assert_eq!(f.update(60, t + Duration::from_secs(2)), 55);
    }
}
