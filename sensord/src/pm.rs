//! Particulate sensor control. The sensor is off at night (`particulate.night` in config.json, or
//! the window set on the screen) and can be switched from the screen, which writes commands to
//! /run/open-cgs2/pm/cmd and reads /run/open-cgs2/pm/status. Its bus is separate from the other
//! sensors', so a sensor that stops answering only delays its own reads.
use crate::{log, state, Shared};
use std::fs;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const SYS: &str = "/sys/bus/i2c/devices/3-0012";
const DIR: &str = "/run/open-cgs2/pm";
/// Readings right after power-on are withheld while the fan spins up.
const WARMUP: u64 = 30;
/// Failed reads in a row before the sensor is reported as not responding.
const FAULT_AFTER: u32 = 5;

/// Night window in minutes after local midnight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub from: u32,
    pub until: u32,
}

impl Window {
    pub fn parse(s: &str) -> Option<Window> {
        let minutes = |t: &str| {
            let (h, m) = t.trim().split_once(':')?;
            let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
            (h < 24 && m < 60).then_some(h * 60 + m)
        };
        let (from, until) = s.split_once('-')?;
        Some(Window { from: minutes(from)?, until: minutes(until)? })
    }

    fn contains(&self, m: u32) -> bool {
        if self.from <= self.until {
            self.from <= m && m < self.until
        } else {
            m >= self.from || m < self.until
        }
    }

    fn minutes_left(&self, m: u32) -> u32 {
        (self.until + 1440 - m) % 1440
    }
}

fn hhmm(m: u32) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// What the user chose; kept across restarts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    pub night: bool,
    /// Turned on during the night window: stays on until this time (seconds since the epoch).
    pub until: i64,
    /// The night window set on the screen, in place of `pm_night`.
    pub window: Option<Window>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { enabled: true, night: true, until: 0, window: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    On,
    Off,
    Night,
}

impl Settings {
    pub fn mode(&self, w: &Window, now: i64, minute: u32) -> Mode {
        if !self.enabled {
            Mode::Off
        } else if self.night && w.contains(minute) && self.until <= now {
            Mode::Night
        } else {
            Mode::On
        }
    }

    pub fn apply(&mut self, cmd: &str, w: &Window, now: i64, minute: u32) {
        match cmd.trim() {
            "power on" => {
                self.enabled = true;
                self.until = if self.night && w.contains(minute) { now + w.minutes_left(minute) as i64 * 60 } else { 0 };
            }
            "power off" => *self = Settings { enabled: false, until: 0, ..*self },
            "night on" => *self = Settings { night: true, until: 0, ..*self },
            "night off" => *self = Settings { night: false, until: 0, ..*self },
            other => match other.strip_prefix("window ").and_then(Window::parse) {
                Some(window) => *self = Settings { window: Some(window), until: 0, ..*self },
                None => log(format!("pm: unknown command {other:?}")),
            },
        }
    }

    fn load() -> Settings {
        let text = fs::read_to_string(state("pm.state")).unwrap_or_default();
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('='));
        let d = Settings::default();
        Settings {
            enabled: get("enabled").map_or(d.enabled, |v| v == "1"),
            night: get("night").map_or(d.night, |v| v == "1"),
            until: get("until").and_then(|v| v.parse().ok()).unwrap_or(0),
            window: get("window").and_then(Window::parse),
        }
    }

    fn save(&self) {
        let window = self.window.map_or(String::new(), |w| format!("window={}-{}\n", hhmm(w.from), hhmm(w.until)));
        let text = format!("enabled={}\nnight={}\nuntil={}\n{window}", self.enabled as u8, self.night as u8, self.until);
        if let Err(e) = fs::write(state("pm.state"), text) {
            log(format!("pm: saving settings: {e}"));
        }
    }
}

fn local_minute(t: i64) -> u32 {
    unsafe {
        let t = t as libc::c_long;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        (tm.tm_hour * 60 + tm.tm_min) as u32
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub pm1: f32,
    pub pm25: f32,
    pub pm10: f32,
    /// As the app reports it: "P9", the first four hex digits, then the last four in decimal.
    pub sn: String,
}

/// A reading, unless the sensor answered with zeros everywhere (just powered or not running).
fn parse(line: &str) -> Option<Reading> {
    let field = |k: &str| line.split(',').find_map(|p| p.trim().strip_prefix(k)?.strip_prefix('=')).map(str::trim);
    let get = |k: &str| field(k)?.parse::<f32>().ok();
    if get("temperature")? == 0.0 && get("humidity")? == 0.0 {
        return None;
    }
    let sn = field("sn").unwrap_or_default();
    let sn = match (sn.get(..4), sn.get(4..8).and_then(|s| u16::from_str_radix(s, 16).ok())) {
        (Some(head), Some(tail)) => format!("P9{head}{tail:04}"),
        _ => String::new(),
    };
    Some(Reading { pm1: get("pm1")?, pm25: get("pm25")?, pm10: get("pm10")?, sn })
}

pub fn power(on: bool) {
    if let Err(e) = fs::write(format!("{SYS}/pm_9500_set_level"), if on { "1" } else { "0" }) {
        log(format!("pm: power {}: {e}", if on { "on" } else { "off" }));
    }
}

fn take_commands() -> Vec<String> {
    let (cmd, taken) = (format!("{DIR}/cmd"), format!("{DIR}/cmd.taken"));
    if fs::rename(&cmd, &taken).is_err() {
        return Vec::new();
    }
    let text = fs::read_to_string(&taken).unwrap_or_default();
    let _ = fs::remove_file(&taken);
    text.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect()
}

fn status_json(state: &str, s: &Settings, w: &Window, last: Option<&Reading>) -> String {
    let values = last.filter(|_| state == "on").map_or(String::new(), |r| format!(",\"pm1\":{},\"pm25\":{},\"pm10\":{}", r.pm1, r.pm25, r.pm10));
    format!(
        "{{\"state\":\"{state}\",\"enabled\":{},\"night\":{},\"from\":\"{}\",\"until\":\"{}\"{values}}}",
        s.enabled,
        s.night,
        hhmm(w.from),
        hhmm(w.until)
    )
}

fn write_status(json: &str) {
    let (path, tmp) = (format!("{DIR}/status"), format!("{DIR}/status.tmp"));
    if let Err(e) = fs::write(&tmp, json).and_then(|_| fs::rename(&tmp, &path)) {
        log(format!("pm: {path}: {e}"));
    }
}

pub fn run(shared: Arc<Shared>) {
    let _ = fs::create_dir_all(DIR);
    let configured = shared.config["particulate"]["night"].as_str().and_then(Window::parse).unwrap_or(Window { from: 22 * 60, until: 8 * 60 + 30 });
    let mut settings = Settings::load();
    let mut powered: Option<bool> = None;
    let mut since = Instant::now();
    let mut failures = 0;
    let mut last: Option<Reading> = None;
    let mut status = String::new();
    loop {
        let tick = Instant::now();
        let now = crate::now();
        let minute = local_minute(now);
        for cmd in take_commands() {
            settings.apply(&cmd, &settings.window.unwrap_or(configured), now, minute);
            settings.save();
            log(format!("pm: {cmd}"));
        }
        let window = settings.window.unwrap_or(configured);
        let mode = settings.mode(&window, now, minute);
        let on = mode == Mode::On;
        if powered != Some(on) {
            power(on);
            log(format!("pm: sensor {}", if on { "on" } else { "off" }));
            powered = Some(on);
            since = Instant::now();
            failures = 0;
            last = None;
            status = status_json(if on { "warming" } else if mode == Mode::Night { "night" } else { "off" }, &settings, &window, None);
            write_status(&status);
        }
        let state = match mode {
            Mode::Off => "off",
            Mode::Night => "night",
            Mode::On => match fs::read_to_string(format!("{SYS}/pm_9500_read")).ok().as_deref().and_then(parse) {
                Some(r) => {
                    failures = 0;
                    if since.elapsed() >= Duration::from_secs(WARMUP) {
                        last = Some(r);
                        "on"
                    } else {
                        "warming"
                    }
                }
                None => {
                    failures += 1;
                    if failures >= FAULT_AFTER {
                        last = None;
                        "fault"
                    } else if since.elapsed() >= Duration::from_secs(WARMUP) {
                        "on"
                    } else {
                        "warming"
                    }
                }
            },
        };
        {
            let mut r = shared.readings.lock().unwrap();
            r.pm = last.clone().filter(|_| state == "on");
            r.pm_state = state;
        }
        let json = status_json(state, &settings, &window, last.as_ref());
        if json != status {
            write_status(&json);
            status = json;
        }
        thread::sleep(Duration::from_secs(1).saturating_sub(tick.elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIGHT: Window = Window { from: 22 * 60, until: 8 * 60 + 30 };

    #[test]
    fn night_window_wraps_midnight() {
        assert_eq!(Window::parse("22:00-08:30"), Some(NIGHT));
        assert!(NIGHT.contains(23 * 60) && NIGHT.contains(60) && !NIGHT.contains(12 * 60) && !NIGHT.contains(8 * 60 + 30));
        assert_eq!(NIGHT.minutes_left(23 * 60), 570);
    }

    #[test]
    fn turning_on_at_night_lasts_until_morning() {
        let mut s = Settings::default();
        let (now, minute) = (100_000, 23 * 60);
        assert_eq!(s.mode(&NIGHT, now, minute), Mode::Night);
        s.apply("power on", &NIGHT, now, minute);
        assert_eq!(s.mode(&NIGHT, now + 3600, minute + 60), Mode::On);
        assert_eq!(s.mode(&NIGHT, now + 570 * 60, 8 * 60 + 30), Mode::On);
        assert_eq!(s.mode(&NIGHT, now + 86400, minute), Mode::Night);
    }

    #[test]
    fn turning_off_holds_through_the_morning() {
        let mut s = Settings::default();
        s.apply("power off", &NIGHT, 0, 12 * 60);
        assert_eq!(s.mode(&NIGHT, 86400, 9 * 60), Mode::Off);
        s.apply("night off", &NIGHT, 0, 23 * 60);
        s.apply("power on", &NIGHT, 0, 23 * 60);
        assert_eq!(s.mode(&NIGHT, 0, 23 * 60), Mode::On);
    }

    #[test]
    fn parses_sensor_line() {
        let r = parse("pm1=3,pm25=4,pm10=5,temperature=616,humidity=96,sn=4480037C\n").unwrap();
        assert_eq!((r.pm1, r.pm25, r.pm10, r.sn.as_str()), (3.0, 4.0, 5.0, "P944800892"));
        assert_eq!(parse(""), None);
        assert_eq!(parse("pm1=0,pm25=0,pm10=0,temperature=0,humidity=0,sn=4480037C"), None);
    }
}
