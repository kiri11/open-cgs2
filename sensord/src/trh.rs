//! Temperature and humidity compensation, ported from the stock app (TRHRunner, Timer and
//! TempHumFixEngine in QingSnow2App). The case warms the sensor: after power-on (`preheating`,
//! up to 55 min), while charging, and with the backlight (`bright`, a slow average). Each tick
//! advances those timers, and the corrected temperature is the raw one plus every model's
//! adjustment. Humidity is then moved to the corrected temperature at the same dew point.
use crate::models::Part;
use std::collections::VecDeque;

/// Seconds per tick; the app's timers count in these steps.
pub const TICK: i32 = 2;
const CHARGING_TIMES: i32 = 5;
const CURRENT_QUEUE: usize = 1;
const BRIGHTNESS_QUEUE: usize = 720;
/// Saved state older than this is not restored.
const RESTORE_WITHIN: i64 = 3600;

pub struct Models {
    preheating: Part,
    charging: Part,
    chargingdone: Part,
    discharging: Part,
    bright: Part,
    wifi: Part,
    sleep: Option<Part>,
    sleeppre: Option<Part>,
}

impl Models {
    pub fn load(text: &str) -> Option<Models> {
        Some(Models {
            preheating: Part::load(text, "preheating")?,
            charging: Part::load(text, "charging")?,
            chargingdone: Part::load(text, "chargingdone")?,
            discharging: Part::load(text, "discharging")?,
            bright: Part::load(text, "bright")?,
            wifi: Part::load(text, "wifi")?,
            sleep: Part::load(text, "sleep"),
            sleeppre: Part::load(text, "sleeppre"),
        })
    }
}

pub struct Battery {
    pub status: String,
    pub current_ma: f32,
    pub capacity: i32,
}

impl Battery {
    pub fn charging(&self) -> bool {
        (self.status == "Full" && self.current_ma > -100.0) || self.status == "Charging"
    }
}

/// What the app saves every 10 s and at shutdown (`temp_fix.ini`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub time: i64,
    pub turnon: i32,
    pub charging: i32,
    pub discharging: i32,
    pub temp_raw: f32,
    pub avg_brightness: f32,
}

impl Saved {
    pub fn text(&self) -> String {
        format!(
            "time={}
turnon={}
charging={}
discharging={}
temp_raw={}
avg_brightness={}
",
            self.time, self.turnon, self.charging, self.discharging, self.temp_raw, self.avg_brightness
        )
    }

    pub fn parse(text: &str) -> Option<Saved> {
        fn get<T: std::str::FromStr>(text: &str, k: &str) -> Option<T> {
            text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('='))?.parse().ok()
        }
        Some(Saved {
            time: get(text, "time")?,
            turnon: get(text, "turnon")?,
            charging: get(text, "charging")?,
            discharging: get(text, "discharging")?,
            temp_raw: get(text, "temp_raw")?,
            avg_brightness: get(text, "avg_brightness")?,
        })
    }
}

/// The inputs of one correction, as the app logs them.
#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub state: i32,
    pub turnon: i32,
    pub charging: i32,
    pub discharging: i32,
    pub brightness: i32,
}

#[derive(Clone, Copy, Default)]
pub struct Terms {
    pub preheat: f32,
    pub charging: f32,
    pub discharging: f32,
    pub bright: f32,
    pub wifi: f32,
}

pub struct Trh {
    m: Models,
    turnon: i32,
    turnon_max: i32,
    charging: f32,
    charging_max: i32,
    done_max: i32,
    discharging: i32,
    discharging_max: i32,
    status: i32,
    currents: VecDeque<i32>,
    current_sum: i32,
    current_max: i32,
    bright: VecDeque<i32>,
    bright_sum: i32,
    bright_ticks: i32,
    pub sample: Sample,
}

impl Trh {
    pub fn new(m: Models, now: i64, battery: &Battery, raw: f32, saved: Option<&Saved>) -> Trh {
        let mut t = Trh {
            turnon: 0,
            turnon_max: m.preheating.max_pos(),
            charging: 0.0,
            charging_max: m.charging.max_pos().max(1),
            done_max: m.chargingdone.max_pos().max(1),
            discharging: 0,
            discharging_max: m.discharging.max_pos(),
            status: 2,
            currents: VecDeque::new(),
            current_sum: 0,
            current_max: 0,
            bright: VecDeque::new(),
            bright_sum: 0,
            bright_ticks: 0,
            sample: Sample::default(),
            m,
        };
        let saved = saved.filter(|s| now - s.time < RESTORE_WITHIN);
        if let Some(s) = saved {
            let elapsed = (now - s.time) as i32;
            t.set_charging(s.charging);
            t.minus_charging(elapsed);
            t.discharging = s.discharging.min(t.discharging_max);
            t.minus_discharging(elapsed);
        }
        let charging = battery.charging();
        let capacity = ((100.0 - (100 - battery.capacity) as f64 / 0.8).round() as i32).max(1);
        let state = match (charging, capacity < 99) {
            (false, _) => 0,
            (true, true) => 1,
            (true, false) => 2,
        };
        t.status_update(state, TICK);
        let Some(s) = saved.filter(|s| s.turnon >= 0) else { return t };
        let elapsed = now - s.time;
        let turnon = if charging { t.turnon_after_sleep(s.turnon, elapsed as i32) } else { t.turnon_after_cooling(s.turnon, raw - s.temp_raw) };
        if elapsed < 300_000 {
            if s.avg_brightness > -1.0 {
                t.sample.brightness = t.brightness_avg(s.avg_brightness as i32);
            }
        } else if elapsed > 1_800_000 {
            t.sample.brightness = t.brightness_avg(100);
        }
        t.turnon = turnon.min(t.turnon_max);
        t
    }

    /// One step of the app's TRHRunner::refreshValue, with the backlight level (0 when dark).
    pub fn tick(&mut self, battery: &Battery, backlight: i32) {
        let state = if !battery.charging() {
            0
        } else if battery.capacity < 99 {
            let current = ((battery.current_ma as f64 * 1000.0) as i32).wrapping_mul(1000);
            let reset = self.status < 1;
            if self.fully_charged(current, reset) {
                2
            } else {
                1
            }
        } else {
            2
        };
        self.status_update(state, TICK);
        self.turnon = (self.turnon + TICK).min(self.turnon_max);
        self.sample = Sample {
            state: self.status,
            turnon: self.turnon,
            charging: self.charging_duration(),
            discharging: self.discharging,
            brightness: self.brightness_avg(backlight),
        };
    }

    /// The state to carry across a restart, with the latest raw temperature.
    pub fn saved(&self, now: i64, temp_raw: f32) -> Saved {
        let avg_brightness = if self.bright.is_empty() { -1.0 } else { (self.bright_sum / self.bright.len() as i32) as f32 };
        Saved { time: now, turnon: self.turnon, charging: self.charging as i32, discharging: self.discharging, temp_raw, avg_brightness }
    }

    pub fn terms(&self) -> Terms {
        let s = &self.sample;
        let charging = if s.state == 1 { &self.m.charging } else { &self.m.chargingdone };
        Terms {
            preheat: self.m.preheating.adjust(s.turnon),
            charging: charging.adjust(s.charging),
            discharging: self.m.discharging.adjust(s.discharging),
            bright: self.m.bright.adjust(s.brightness),
            wifi: self.m.wifi.adjust(1),
        }
    }

    /// The corrected temperature before rounding, summed in the app's order.
    pub fn fix(&self, raw: f32) -> f32 {
        let t = self.terms();
        t.wifi + t.bright + t.discharging + t.charging + raw + t.preheat
    }

    fn status_update(&mut self, state: i32, dt: i32) {
        match state {
            0 => {
                if self.status == 1 {
                    let d = self.charging_done_duration(self.charging_duration());
                    self.set_charging(d);
                }
                self.discharging = (self.discharging + dt).min(self.discharging_max);
                self.minus_charging(dt);
            }
            1 => {
                if self.status != 1 {
                    let d = self.charging_duration_from(self.charging_duration());
                    self.set_charging(d);
                }
                self.charging = (self.charging + dt as f32).min(self.charging_max as f32);
                self.minus_discharging(dt * CHARGING_TIMES);
            }
            2 => {
                if self.status == 1 {
                    let d = self.charging_done_duration(self.charging_duration());
                    self.set_charging(d);
                }
                self.minus_charging(dt);
                self.minus_discharging(dt);
            }
            _ => {}
        }
        self.status = state;
    }

    fn set_charging(&mut self, d: i32) {
        self.charging = (d as f32).min(self.charging_max as f32);
    }

    fn minus_charging(&mut self, dt: i32) {
        self.charging = (self.charging - self.charging_max as f32 * dt as f32 / self.done_max as f32).max(0.0);
    }

    fn minus_discharging(&mut self, dt: i32) {
        self.discharging = (self.discharging - 2 * dt).max(0);
    }

    fn charging_duration(&self) -> i32 {
        if self.status == 1 {
            self.charging as i32
        } else {
            (self.done_max as f32 - self.charging * self.done_max as f32 / self.charging_max as f32) as i32
        }
    }

    fn charging_done_duration(&self, d: i32) -> i32 {
        if d >= self.charging_max {
            return self.charging_max;
        }
        let at = self.m.charging.adjust(d);
        for u in (0..self.done_max).step_by(10) {
            if at < self.m.chargingdone.adjust(u) {
                return ((self.done_max - u) as f32 * self.charging_max as f32 / self.done_max as f32) as i32;
            }
        }
        d
    }

    fn charging_duration_from(&self, d: i32) -> i32 {
        if d == 0 || d >= self.charging_max {
            return 0;
        }
        let at = self.m.chargingdone.adjust(d);
        (0..self.charging_max).step_by(10).find(|&u| self.m.charging.adjust(u) < at).unwrap_or(d)
    }

    fn fully_charged(&mut self, current: i32, reset: bool) -> bool {
        if reset {
            self.currents.clear();
            self.current_sum = 0;
        }
        if self.currents.len() >= CURRENT_QUEUE {
            if let Some(v) = self.currents.pop_front() {
                self.current_sum = self.current_sum.wrapping_sub(v);
            }
        }
        self.currents.push_back(current);
        self.current_sum = self.current_sum.wrapping_add(current);
        if self.currents.len() >= CURRENT_QUEUE {
            let avg = self.current_sum / CURRENT_QUEUE as i32;
            if avg <= self.current_max {
                return avg / 1000 < self.current_max / 1000;
            }
            self.current_max = avg;
        }
        false
    }

    fn push_brightness(&mut self, b: i32) {
        if self.bright.len() >= BRIGHTNESS_QUEUE {
            if let Some(v) = self.bright.pop_front() {
                self.bright_sum -= v;
            }
        }
        self.bright.push_back(b);
        self.bright_sum += b;
    }

    fn brightness_avg(&mut self, b: i32) -> i32 {
        if self.bright.is_empty() {
            for _ in 0..BRIGHTNESS_QUEUE / 3 * 2 {
                self.bright.push_back(b);
                self.bright_sum += b;
            }
        }
        if self.bright_ticks == 120 {
            self.bright_ticks = 0;
        }
        self.bright_ticks += 1;
        let mut avg = self.bright_sum / self.bright.len() as i32;
        let (first, last) = (self.bright[0], self.bright[self.bright.len() - 1]);
        let spread = if first == last { 0.5 } else { (avg - b).abs() as f32 / (first - last).abs() as f32 };
        if spread > 0.7 {
            self.push_brightness(b);
        }
        if spread >= 0.3 || self.bright_ticks % 5 == 0 {
            self.push_brightness(b);
            avg = self.bright_sum / self.bright.len() as i32;
        }
        avg
    }

    fn turnon_after_cooling(&self, prev: i32, diff: f32) -> i32 {
        if !(diff < 0.0) {
            return prev;
        }
        let target = self.m.preheating.adjust(prev) - diff;
        if target >= 0.0 {
            return 0;
        }
        (0..prev.max(0)).step_by(60).find(|&u| self.m.preheating.adjust(u) < target).unwrap_or(prev)
    }

    fn turnon_after_sleep(&self, prev: i32, elapsed: i32) -> i32 {
        let sleep = self.m.sleep.as_ref().map_or(0.0, |p| p.adjust(elapsed)).min(0.0);
        let pre = self.m.sleeppre.as_ref().map_or(0.0, |p| p.adjust(elapsed)).max(0.0);
        let turned = self.m.preheating.adjust(prev);
        let target = if -pre < turned - sleep { -pre } else { turned - sleep };
        if target >= 0.0 {
            return 0;
        }
        (0..=self.turnon_max).step_by(60).find(|&u| self.m.preheating.adjust(u) < target).unwrap_or(self.turnon_max)
    }
}

/// The app's rounding to one decimal (QString::number(x, 'f', 1)).
pub fn round1(x: f32) -> f32 {
    format!("{:.1}", x as f64).parse().unwrap_or(x)
}

pub fn temperature(fixed: f32) -> f32 {
    round1(fixed.clamp(-20.0, 50.0))
}

/// Raw humidity moved from the raw to the corrected temperature at the same dew point (Magnus).
pub fn humidity(raw_t: f32, raw_h: f32, fixed: f32) -> f32 {
    let h = if raw_t != fixed {
        (raw_h as f64 * (((raw_t - fixed) as f64 * 4283.78 / (fixed as f64 + 243.12)) / (raw_t as f64 + 243.12)).exp()) as f32
    } else {
        raw_h
    };
    round1(h.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::STOCK;

    fn on_battery() -> Battery {
        Battery { status: "Discharging".into(), current_ma: -170.0, capacity: 44 }
    }

    fn warm(brightness: i32) -> Trh {
        let mut t = Trh::new(Models::load(STOCK).unwrap(), 0, &on_battery(), 0.0, None);
        for _ in 0..2000 {
            t.tick(&on_battery(), brightness);
        }
        t
    }

    // Raw readings, corrected values and inputs as the app logged them on 2026-10-10.
    #[test]
    fn matches_app_log() {
        let t = warm(91);
        assert_eq!(t.sample.turnon, 3300);
        assert_eq!(t.sample.brightness, 91);
        assert!((t.fix(23.2) - 21.3947).abs() < 1e-4);
        assert!((warm(21).fix(21.7) - 20.1152).abs() < 1e-4);
    }

    #[test]
    fn restores_recent_state() {
        let saved = Saved { time: 1000, turnon: 3300, charging: 0, discharging: 3600, temp_raw: 22.3, avg_brightness: 296.0 };
        let t = Trh::new(Models::load(STOCK).unwrap(), 1010, &on_battery(), 22.3, Some(&saved));
        assert_eq!(t.turnon, 3300);
        assert_eq!(t.sample.brightness, 296);
        let cooled = Trh::new(Models::load(STOCK).unwrap(), 1010, &on_battery(), 20.0, Some(&saved));
        assert_eq!(cooled.turnon, 0);
        let stale = Trh::new(Models::load(STOCK).unwrap(), 1000 + RESTORE_WITHIN, &on_battery(), 22.3, Some(&saved));
        assert_eq!(stale.turnon, 0);
        let again = t.saved(1010, 22.3);
        assert_eq!((again.turnon, again.avg_brightness), (3300, 296.0));
        assert_eq!(Saved::parse(&again.text()), Some(again));
    }
}
