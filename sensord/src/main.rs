//! Sensor daemon for the CGS2, in place of the stock app (QingSnow2App). It reads the sensors,
//! corrects temperature and humidity for the case's own heat as the app does, computes the VOC
//! index, owns the particulate sensor, feeds the hardware watchdog, shuts down on a flat battery
//! and publishes the readings to Home Assistant over MQTT discovery. Unlike the app it keeps the
//! VOC baseline across restarts.
mod calibration;
mod ha;
mod hourly;
mod i2c;
mod models;
mod mqtt;
mod pm;
mod sensors;
mod trh;
mod voc;

use serde_json::Value;

use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MODELS: &str = "/qingping/etc/models.ini";
const APP_STATE: &str = "/qingping/etc/temp_fix.ini";
const BATTERY: &str = "/sys/class/power_supply/battery";
const WATCHDOG: &str = "/dev/watchdog";
pub const STATE_DIR: &str = "/userdata/open-cgs2";
pub const RUN_DIR: &str = "/run/open-cgs2";
const CONFIG: &str = "/userdata/open-cgs2/config.json";
const STATUS: &str = "/run/open-cgs2/sensors";
pub const CSV: &str = "/userdata/open-cgs2/sensord.csv";
/// The minute log moves to .1 once its first row is this old, so 2 to 4 weeks are kept.
const CSV_KEEP: i64 = 14 * 86400;
/// A backstop in case the clock stops moving.
const CSV_MAX: u64 = 8 << 20;
/// Times before this are a clock not yet set (some units start in 2018); such rows are not logged.
pub const MIN_TIME: i64 = 1767225600;
const CSV_HEADER: &str = "time,temp,humidity,co2,voc,light,pm25,pm10,raw_t,raw_h,scd_t,scd_h,sraw,voc_index,\
preheat,charging,bright,turnon,chg,dis,brightness,state,backlight,bat_status,bat_ma,bat_cap";
/// Seconds between saves of the learned state, which is also saved on exit.
const SAVE_EVERY: u64 = 600;
/// The watchdog goes unfed, and the device reboots, once the sensors stop updating this long.
const STALL: u64 = 60;
/// Seconds between copies of the system clock (set by NTP) to the RTC, which nothing else writes.
const RTC_EVERY: u64 = 6 * 3600;

static STOP: AtomicBool = AtomicBool::new(false);
/// The time of the minute log's first row, 0 until read.
static CSV_FIRST: AtomicI64 = AtomicI64::new(0);

pub fn log(msg: impl AsRef<str>) {
    eprintln!("{} {}", now(), msg.as_ref());
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_num<T: std::str::FromStr>(path: &str) -> Option<T> {
    read(path)?.parse().ok()
}

pub fn state(name: &str) -> String {
    format!("{STATE_DIR}/{name}")
}

/// Writes through a temporary file, so readers never see half a file.
pub fn replace(path: &str, text: &str) -> io::Result<()> {
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)
}

pub fn battery() -> trh::Battery {
    let capacity = read_num(&format!("{BATTERY}/capacity")).unwrap_or(0);
    let current: f64 = read_num(&format!("{BATTERY}/current_now")).unwrap_or(0.0);
    trh::Battery {
        status: read(&format!("{BATTERY}/status")).unwrap_or_default(),
        current_ma: format!("{:.2}", current / 1000.0).parse().unwrap_or(0.0),
        capacity: if capacity > 100 { 66 } else { capacity },
    }
}

/// The percentage the app shows and reports: the kernel's 20..100 spread over 1..100.
pub fn reported_capacity(raw: i32) -> i32 {
    ((100.0 - (100 - raw) as f64 / 0.8).round() as i32).max(1)
}

/// Values from a QSettings ini file, by section; floats are stored as escaped `@Variant` bytes.
pub struct Ini(Vec<(String, String, String)>);

impl Ini {
    pub fn read(path: &str) -> Option<Ini> {
        let text = fs::read_to_string(path).ok()?;
        let mut section = String::new();
        let mut values = Vec::new();
        for line in text.lines().map(str::trim) {
            if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = s.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                values.push((section.clone(), k.trim().to_string(), v.trim().to_string()));
            }
        }
        Some(Ini(values))
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.0.iter().find(|(s, k, _)| s == section && k == key).map(|(_, _, v)| v.as_str())
    }

    fn float(&self, key: &str) -> Option<f32> {
        let v = self.get("General", key)?;
        let Some(inner) = v.strip_prefix("@Variant(").and_then(|v| v.strip_suffix(')')) else { return v.parse().ok() };
        let b = unescape(inner);
        (b.len() == 8 && b[..4] == [0, 0, 0, 0x87]).then(|| f32::from_be_bytes([b[4], b[5], b[6], b[7]]))
    }
}

fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'x' {
                let mut j = i + 2;
                let mut v = 0u32;
                while j < b.len() && j < i + 6 && b[j].is_ascii_hexdigit() {
                    v = v * 16 + (b[j] as char).to_digit(16).unwrap();
                    j += 1;
                }
                out.push(v as u8);
                i = j;
                continue;
            }
            out.push(match b[i + 1] {
                b'0' => 0,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                c => c,
            });
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The state the app left in temp_fix.ini, used when sensord has none of its own yet.
fn app_saved(ini: &Ini) -> Option<trh::Saved> {
    let info = ini.get("General", "LAST_TEMP_FIX_INFO")?.trim_matches('"');
    let field = |k: &str| {
        info.split('&').find_map(|p| p.strip_prefix(k)?.strip_prefix('=')).and_then(|v| v.parse::<f32>().ok()).map(|v| v as i32)
    };
    Some(trh::Saved {
        time: ini.get("General", "POWEROFF_TIME")?.parse().ok()?,
        turnon: field("turnOn")?,
        charging: field("chg").unwrap_or(0),
        discharging: field("dischargingDuration").unwrap_or(0),
        temp_raw: ini.float("LAST_POWEROFF_TEMP_RAW")?,
        avg_brightness: ini.float("LAST_POWEROFF_AVG_LIGHTNESS").unwrap_or(-1.0),
    })
}

#[derive(Default)]
pub struct Readings {
    pub temperature: Option<f32>,
    pub humidity: Option<f32>,
    pub co2: Option<i32>,
    pub voc: Option<i32>,
    pub light: Option<i32>,
    pub pm: Option<pm::Reading>,
    /// The particulate sensor's state as the screen shows it ("on", "warming", "off", ...).
    pub pm_state: &'static str,
    pub raw: sensors::Raw,
}

pub struct Shared {
    /// config.json, shared with the screen.
    pub config: Value,
    pub readings: Mutex<Readings>,
    pub voc_state: Mutex<String>,
    pub trh_state: Mutex<Option<trh::Saved>>,
    pub calibration: Mutex<calibration::Calibration>,
    /// Counts the sensor thread's rounds, for the watchdog.
    pub heartbeat: AtomicU64,
}

fn load_config() -> Value {
    let path = std::env::args().nth(1).unwrap_or_else(|| CONFIG.into());
    match fs::read_to_string(&path).map(|t| serde_json::from_str(&t)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            log(format!("{path}: {e}"));
            Value::Null
        }
        Err(_) => Value::Null,
    }
}

fn start_trh() -> trh::Trh {
    let models = read(MODELS).and_then(|t| trh::Models::load(&t)).expect("load models.ini");
    let raw = sensors::temperature();
    let ours = read(&state("trh.state")).and_then(|t| trh::Saved::parse(&t));
    let saved = ours.or_else(|| Ini::read(APP_STATE).as_ref().and_then(app_saved));
    let trh = trh::Trh::new(models, now(), &battery(), raw.or(saved.map(|s| s.temp_raw)).unwrap_or(0.0), saved.as_ref());
    log(format!(
        "temperature state: {}",
        saved.map_or("none".into(), |s| format!("turnOn={} chg={} dis={} from {} s ago", s.turnon, s.charging, s.discharging, now() - s.time))
    ));
    trh
}

fn start_voc() -> (voc::Voc, Option<voc::Warmup>) {
    let v = match read(&state("voc.state")).and_then(|t| voc::Voc::restored(&t)) {
        Some(v) => {
            let age = fs::metadata(state("voc.state")).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
            log(format!("VOC baseline restored, saved {} s ago", age.map_or(0, |a| a.as_secs())));
            v
        }
        None => {
            log("VOC baseline: none saved, learning from scratch");
            voc::Voc::new()
        }
    };
    let uptime: f32 = read("/proc/uptime").and_then(|u| u.split_whitespace().next()?.parse().ok()).unwrap_or(0.0);
    let warmup = (uptime < voc::SETTLE_MAX).then(|| {
        log("VOC index held until the sensor has warmed up");
        voc::Warmup::new(uptime)
    });
    (v, warmup)
}

fn save(shared: &Shared) {
    let voc = shared.voc_state.lock().unwrap().clone();
    if !voc.is_empty() {
        if let Err(e) = replace(&state("voc.state"), &voc) {
            log(format!("saving the VOC baseline: {e}"));
        }
    }
    let trh = *shared.trh_state.lock().unwrap();
    if let Some(s) = trh {
        if let Err(e) = replace(&state("trh.state"), &s.text()) {
            log(format!("saving the temperature state: {e}"));
        }
    }
}

/// The hardware watchdog, opened once; it reboots the device if nothing writes to it in time.
fn open_watchdog() -> Option<fs::File> {
    const WDIOC_GETTIMEOUT: libc::c_ulong = 0x80045707;
    match fs::OpenOptions::new().write(true).open(WATCHDOG) {
        Ok(f) => {
            let mut timeout: libc::c_int = 0;
            if unsafe { libc::ioctl(f.as_raw_fd(), WDIOC_GETTIMEOUT as _, &mut timeout) } == 0 {
                log(format!("watchdog timeout {timeout} s"));
            }
            Some(f)
        }
        Err(e) => {
            log(format!("{WATCHDOG}: {e}"));
            None
        }
    }
}

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// A CSV log that moves to `<path>.1` past CSV_MAX.
fn append_csv(row: &str) {
    let t = now();
    if t < MIN_TIME {
        return;
    }
    let mut first = CSV_FIRST.load(Ordering::Relaxed);
    if first == 0 {
        first = hourly::first_time(CSV).unwrap_or(t);
    }
    if t - first > CSV_KEEP || fs::metadata(CSV).is_ok_and(|m| m.len() > CSV_MAX) {
        let _ = fs::rename(CSV, format!("{CSV}.1"));
        first = t;
    }
    CSV_FIRST.store(first, Ordering::Relaxed);
    let new = fs::metadata(CSV).is_err();
    let res = fs::OpenOptions::new().create(true).append(true).open(CSV).and_then(|mut f| {
        if new {
            writeln!(f, "{CSV_HEADER}")?;
        }
        writeln!(f, "{row}")
    });
    if let Err(e) = res {
        log(format!("{CSV}: {e}"));
    }
}

fn csv_row(r: &Readings, bat: &trh::Battery, backlight: i32) -> String {
    let o = |v: Option<f32>| v.map_or(String::new(), |v| format!("{v}"));
    let i = |v: Option<i32>| v.map_or(String::new(), |v| v.to_string());
    let (x, s) = (&r.raw, &r.raw.sample);
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{}",
        now(),
        o(r.temperature),
        o(r.humidity),
        i(r.co2),
        i(r.voc),
        i(r.light),
        o(r.pm.as_ref().map(|p| p.pm25)),
        o(r.pm.as_ref().map(|p| p.pm10)),
        o(x.temperature),
        o(x.humidity),
        o(x.scd_temperature),
        o(x.scd_humidity),
        o(x.sraw),
        x.voc_index.map_or(String::new(), |v| format!("{v:.2}")),
        x.terms.preheat,
        x.terms.charging,
        x.terms.bright,
        s.turnon,
        s.charging,
        s.discharging,
        s.brightness,
        s.state,
        backlight,
        bat.status,
        bat.current_ma,
        bat.capacity,
    )
}

/// The readings rounded as the screen shows them and HA receives them.
pub fn status(r: &Readings, bat: &trh::Battery) -> Value {
    let round1 = |v: f32| (v * 10.0).round() / 10.0;
    serde_json::json!({
        "temperature": r.temperature.map(round1),
        "humidity": r.humidity.map(|h| round1(h.clamp(0.0, 100.0))),
        "co2": r.co2,
        "tvoc": r.voc,
        "illuminance": r.light,
        "pm25": r.pm.as_ref().map(|p| p.pm25),
        "pm10": r.pm.as_ref().map(|p| p.pm10),
        "battery": reported_capacity(bat.capacity),
        "charging": bat.charging(),
    })
}

/// The app powers off below 20% raw (1% shown) when not charging; this does the same once a few
/// readings agree.
fn low_battery(bat: &trh::Battery, count: &mut u32) -> bool {
    if !bat.charging() && bat.capacity < 20 {
        *count += 1;
    } else {
        *count = 0;
    }
    *count >= 10
}

fn main() {
    log("sensord started");
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let _ = fs::create_dir_all(RUN_DIR);
    let _ = fs::create_dir_all(STATE_DIR);
    let config = load_config();
    let shared = Arc::new(Shared {
        calibration: Mutex::new(calibration::Calibration::load(&config)),
        config,
        readings: Mutex::new(Readings::default()),
        voc_state: Mutex::new(String::new()),
        trh_state: Mutex::new(None),
        heartbeat: AtomicU64::new(0),
    });
    let mut watchdog = open_watchdog();
    let (trh, (voc, warmup)) = (start_trh(), start_voc());
    {
        let shared = shared.clone();
        thread::spawn(move || sensors::run(shared, trh, voc, warmup));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || pm::run(shared));
    }
    {
        let shared = shared.clone();
        thread::spawn(move || ha::run(shared));
    }
    let start = Instant::now();
    let mut shown = String::new();
    let (mut beat, mut beat_at) = (0, Instant::now());
    let mut flat = 0;
    for second in 1u64.. {
        let next = start + Duration::from_secs(second);
        thread::sleep(next.saturating_duration_since(Instant::now()));
        if STOP.load(Ordering::Relaxed) {
            save(&shared);
            log("sensord stopped");
            return;
        }
        let b = shared.heartbeat.load(Ordering::Relaxed);
        if b != beat {
            (beat, beat_at) = (b, Instant::now());
        }
        if beat_at.elapsed() < Duration::from_secs(STALL) {
            if let Some(f) = watchdog.as_mut() {
                if let Err(e) = f.write_all(b"1").and_then(|_| f.flush()) {
                    log(format!("{WATCHDOG}: {e}"));
                }
            }
        } else if beat_at.elapsed().as_secs() == STALL {
            log("the sensors stopped updating, leaving the watchdog to reboot");
            save(&shared);
            watchdog.take();
        }
        let bat = battery();
        let json = status(&shared.readings.lock().unwrap(), &bat).to_string();
        if json != shown {
            if let Err(e) = replace(STATUS, &json) {
                log(format!("{STATUS}: {e}"));
            }
            shown = json;
        }
        if second % 60 == 0 {
            let backlight = read_num(sensors::BACKLIGHT).unwrap_or(0);
            append_csv(&csv_row(&shared.readings.lock().unwrap(), &bat, backlight));
            hourly::update(now());
        }
        if second % SAVE_EVERY == 0 {
            save(&shared);
        }
        if second % RTC_EVERY == 60 {
            if let Err(e) = Command::new("hwclock").args(["-w", "-u"]).status() {
                log(format!("hwclock: {e}"));
            }
        }
        if low_battery(&bat, &mut flat) {
            log("battery empty, powering off");
            save(&shared);
            pm::power(false);
            unsafe { libc::sync() };
            let _ = Command::new("poweroff").status();
            thread::sleep(Duration::from_secs(60));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_app_floats() {
        let ini = Ini(vec![
            ("General".into(), "LAST_POWEROFF_HUM".into(), r"@Variant(\0\0\0\x87\x42\x82\xcc\xcd)".into()),
            ("General".into(), "LAST_POWEROFF_TEMP_RAW".into(), r"@Variant(\0\0\0\x87\x41\xb2\x66\x66)".into()),
            ("General".into(), "LAST_POWEROFF_AVG_LIGHTNESS".into(), "296".into()),
        ]);
        assert!((ini.float("LAST_POWEROFF_HUM").unwrap() - 65.4).abs() < 1e-4);
        assert!((ini.float("LAST_POWEROFF_TEMP_RAW").unwrap() - 22.3).abs() < 1e-4);
        assert_eq!(ini.float("LAST_POWEROFF_AVG_LIGHTNESS"), Some(296.0));
    }
}
