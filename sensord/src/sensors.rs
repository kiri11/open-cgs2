//! The sensors on the first I2C bus, read through i2c-dev in one thread: temperature and humidity
//! (SHT4x) every 2 s and VOC (SGP40) and light (OPT3004) every second, as the app did, and CO2
//! (SCD40) whenever it has a new sample (every 5 s). Values keep the sensors' full resolution;
//! the vendor drivers cut them to tenths, and kept the CPU busy while the sensors measured.
use crate::i2c::{self, Dev};
use crate::{battery, log, now, read, replace, state, trh, voc, Shared};
use std::fs;
use std::io;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const BACKLIGHT: &str = "/sys/class/backlight/backlight/brightness";
/// "calibrate <ppm>", written by the screen, asks for a forced recalibration of the SCD40; the
/// result goes to co2.state.
const CO2_CMD: &str = "/run/open-cgs2/co2/cmd";
/// The SCD40 needs this long measuring the reference air before a forced recalibration.
const FRC_AFTER: Duration = Duration::from_secs(180);
/// Seconds without a CO2 sample before the SCD40 is restarted (and again every as many more)
/// and the reading dropped.
const CO2_STALE: u64 = 15;
/// Reads in a row without a value before a reading is dropped, as in the app.
const FAILURES: u32 = 5;
/// What the SGP40 is compensated with until the SCD40 has a sample: its defaults, 25 °C and 50%.
const SGP_DEFAULT: (f32, f32) = (25.0, 50.0);
/// The driver reported the OPT3004 at ten times its datasheet lux, presumably for its window;
/// the screen's brightness curve and HA's history are on that scale.
const LUX_SCALE: f32 = 10.0;

/// What the sensors gave last, before corrections, for the log.
#[derive(Clone, Copy, Default)]
pub struct Raw {
    pub temperature: Option<f32>,
    pub humidity: Option<f32>,
    pub scd_temperature: Option<f32>,
    pub scd_humidity: Option<f32>,
    pub sraw: Option<f32>,

    pub voc_index: Option<f32>,
    pub terms: trh::Terms,
    pub sample: trh::Sample,
}

/// Counts failed reads in a row, logs the first, and says when to give up on a reading.
#[derive(Default)]
struct Failures(u32);

impl Failures {
    fn ok(&mut self) {
        self.0 = 0;
    }

    /// Records a failure; true once there have been more than FAILURES in a row.
    fn failed(&mut self, what: &str, e: &io::Error) -> bool {
        if self.0 == 0 {
            log(format!("{what}: {e}"));
        }
        self.0 += 1;
        self.0 > FAILURES
    }
}

fn take_calibration() -> Option<u16> {
    let taken = format!("{CO2_CMD}.taken");
    fs::rename(CO2_CMD, &taken).ok()?;
    let text = fs::read_to_string(&taken).unwrap_or_default();
    let _ = fs::remove_file(&taken);
    text.trim().strip_prefix("calibrate ")?.trim().parse().ok().filter(|p| (400..=2000).contains(p))
}

fn calibrate(d: &mut Dev, ppm: u16, measuring: Duration) {
    let res = if measuring < FRC_AFTER {
        Err(io::Error::new(io::ErrorKind::Other, "the sensor has measured for less than 3 minutes"))
    } else {
        i2c::scd_frc(d, ppm)
    };
    let outcome = match &res {
        Ok(c) => format!("correction={c}"),
        Err(e) => format!("error={e}"),
    };
    log(format!("CO2 calibration to {ppm} ppm: {outcome}"));
    if let Err(e) = replace(&state("co2.state"), &format!("time={}\nppm={ppm}\n{outcome}\n", now())) {
        log(format!("co2.state: {e}"));
    }
}

/// The SHT4x's temperature in °C, for the temperature correction's start.
pub fn temperature() -> Option<f32> {
    Dev::open(i2c::SHT).and_then(|mut d| i2c::sht(&mut d)).ok().map(|(t, _)| t)
}

pub fn run(shared: Arc<Shared>, mut trh: trh::Trh, mut voc: voc::Voc, mut warmup: Option<voc::Warmup>) {
    let open = |addr| Dev::open(addr).expect("open /dev/i2c-1");
    let (mut sht, mut opt, mut sgp, mut scd) = (open(i2c::SHT), open(i2c::OPT), open(i2c::SGP), open(i2c::SCD));
    log(format!("sensors: {}", i2c::start()));
    let (mut sht_fail, mut light_fail, mut voc_fail) = (Failures::default(), Failures::default(), Failures::default());
    let mut scd_error = false;
    let mut compensation = SGP_DEFAULT;
    let mut co2_at = 0;
    let start = Instant::now();
    for second in 0u64.. {
        if let Some(ppm) = take_calibration() {
            calibrate(&mut scd, ppm, start.elapsed());
            co2_at = second;
        }
        let sample = i2c::scd_ready(&mut scd).and_then(|ready| if ready { i2c::scd(&mut scd).map(Some) } else { Ok(None) });
        if let Ok(Some((ppm, t, h))) = sample {
            co2_at = second;
            scd_error = false;
            if (0.0..=100.0).contains(&t) && (0.0..=100.0).contains(&h) {
                compensation = (t, h);
            }
            let mut r = shared.readings.lock().unwrap();
            r.co2 = Some((ppm as i32).clamp(400, 9999));
            (r.raw.scd_temperature, r.raw.scd_humidity) = (Some(t), Some(h));
        } else {
            if let Err(e) = sample {
                if !scd_error {
                    log(format!("SCD40: {e}"));
                }
                scd_error = true;
            }
            let stale = second - co2_at;
            if stale > 0 && stale % CO2_STALE == 0 {
                log("CO2 sensor not answering, restarting it");
                if let Err(e) = i2c::scd_restart(&mut scd) {
                    log(format!("SCD40 restart: {e}"));
                }
            }
            if stale > CO2_STALE && start.elapsed() > Duration::from_secs(30) {
                shared.readings.lock().unwrap().co2 = None;
            }
        }
        if second % 10 == 0 {
            shared.calibration.lock().unwrap().reload();
        }
        if second % trh::TICK as u64 == 0 {
            trh.tick(&battery(), read(BACKLIGHT).and_then(|b| b.parse().ok()).unwrap_or(0));
            let measured = i2c::sht(&mut sht);
            let mut r = shared.readings.lock().unwrap();
            r.raw.terms = trh.terms();
            r.raw.sample = trh.sample;
            match measured {
                Ok((t, h)) => {
                    sht_fail.ok();
                    let c = shared.calibration.lock().unwrap().values;
                    let fixed = trh.fix(t);
                    r.temperature = Some(trh::temperature(fixed) + c.temperature_offset);
                    r.humidity = Some((trh::humidity(t, (h * c.humidity_scale).min(100.0), fixed) + c.humidity_offset).clamp(0.0, 100.0));
                    (r.raw.temperature, r.raw.humidity) = (Some(t), Some(h));
                    *shared.trh_state.lock().unwrap() = Some(trh.saved(now(), t));
                }
                Err(e) if sht_fail.failed("SHT4x", &e) => (r.temperature, r.humidity) = (None, None),
                Err(_) => {}
            }
        }
        match i2c::lux(&mut opt) {
            Ok(l) => {
                light_fail.ok();
                shared.readings.lock().unwrap().light = Some((l * LUX_SCALE).round() as i32);
            }
            Err(e) if light_fail.failed("OPT3004", &e) => shared.readings.lock().unwrap().light = None,
            Err(_) => {}
        }
        match i2c::sgp(&mut sgp, compensation.1, compensation.0) {
            Ok(sraw) => {
                voc_fail.ok();
                let held = warmup.as_mut().is_some_and(|w| w.holding(sraw));
                if !held {
                    if let Some(w) = warmup.take() {
                        log(format!("VOC sensor warmed up {:.0} s after power-on", w.uptime()));
                    }
                }
                let warming = held || voc.warming();
                let index = (!held).then(|| voc.process(sraw));
                let mut r = shared.readings.lock().unwrap();
                r.voc = index.filter(|_| !warming).map(voc::reported);
                r.raw.sraw = Some(sraw);
                r.raw.voc_index = index;
                *shared.voc_state.lock().unwrap() = voc.state();
            }
            Err(e) if voc_fail.failed("SGP40", &e) => shared.readings.lock().unwrap().voc = None,
            Err(_) => {}
        }
        shared.heartbeat.fetch_add(1, Ordering::Relaxed);
        let next = start + Duration::from_secs(second + 1);
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}
