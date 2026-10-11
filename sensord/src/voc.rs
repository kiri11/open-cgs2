//! VOC index from the SGP40's raw signal, ported from the engine linked into the stock app
//! (SensirionVocEngine, "custom build"): Sensirion's VOC algorithm in floats with Qingping's
//! tuning (initial spread 500, index offset 87, sigmoid centred on 300), stepped once per raw value,
//! then the app's own remapping of the 0..500 index. Parameter names follow Sensirion's. Unlike the
//! app, sensord keeps the learned baseline across restarts (`state`, `restored`): after a restart
//! an engine learning from scratch reports about average air whatever the air does, for a day or
//! more.

use std::collections::VecDeque;

const SAMPLING: f32 = 1.0;
const BLACKOUT: f32 = 45.0;
const TAU_MEAN: f32 = 43200.0;
const TAU_INITIAL_MEAN: f32 = 20.0;
const TAU_INITIAL_VARIANCE: f32 = 2500.0;
const INIT_DURATION_MEAN: f32 = 2700.0;
const INIT_DURATION_VARIANCE: f32 = 5220.0;
const INIT_TRANSITION: f32 = 0.01;
const GATING_THRESHOLD: f32 = 340.0;
const GATING_THRESHOLD_INITIAL: f32 = 510.0;
const GATING_THRESHOLD_TRANSITION: f32 = 0.09;
const GATING_MAX_DURATION: f32 = 10800.0;
const GATING_MAX_RATIO: f32 = 0.3;
const STD_INITIAL: f32 = 500.0;
const STD_BONUS: f32 = 220.0;
const GAIN: f32 = 230.0;
const INDEX_OFFSET: f32 = 87.0;
const SIGMOID_L: f32 = 500.0;
const SIGMOID_K: f32 = -0.0065;
const SIGMOID_X0: f32 = 300.0;
const LP_TAU_FAST: f32 = 20.0;
const LP_TAU_SLOW: f32 = 500.0;
const LP_ALPHA: f32 = -0.2;
const LP_UNSET: f32 = -100.0;

pub struct Voc {
    uptime: f32,
    blackout: f32,
    sraw: f32,
    index: f32,
    initialized: bool,
    offset: f32,
    mean: f32,
    variance: f32,
    uptime_gamma: f32,
    uptime_gating: f32,
    gating_duration: f32,
    gamma_mean: f32,
    gamma_variance: f32,
    mox_mean: f32,
    mox_std: f32,
    lp_fast: f32,
    lp_slow: f32,
    lp_out: f32,
}

impl Default for Voc {
    fn default() -> Voc {
        Voc::new()
    }
}

fn sigmoid(x: f32, l: f32, x0: f32, k: f32) -> f32 {
    let v = (x - x0) * k;
    if v < -50.0 {
        l
    } else if v <= 50.0 {
        l / (v.exp() + 1.0)
    } else {
        0.0
    }
}

impl Voc {
    pub fn new() -> Voc {
        Voc {
            uptime: 0.0,
            blackout: BLACKOUT,
            sraw: 0.0,
            index: 0.0,
            initialized: false,
            offset: 0.0,
            mean: 0.0,
            variance: STD_INITIAL * STD_INITIAL,
            uptime_gamma: 0.0,
            uptime_gating: 0.0,
            gating_duration: 0.0,
            gamma_mean: 0.0,
            gamma_variance: 0.0,
            mox_mean: 0.0,
            mox_std: STD_INITIAL,
            lp_fast: LP_UNSET,
            lp_slow: LP_UNSET,
            lp_out: LP_UNSET,
        }
    }

    /// Whether the engine is still in its blackout, ignoring the raw signal.
    pub fn warming(&self) -> bool {
        self.uptime <= self.blackout
    }

    /// One second's raw signal; returns the unscaled index (0 during the blackout).
    pub fn process(&mut self, sraw: f32) -> f32 {
        if self.warming() {
            self.uptime += SAMPLING;
            return self.index;
        }
        let valid = sraw > 0.0 && sraw < 65000.0;
        if valid {
            self.sraw = sraw;
        }
        let mox = INDEX_OFFSET + GAIN * (-(self.sraw - self.mox_mean) / (self.mox_std + STD_BONUS));
        let scaled = sigmoid(mox, SIGMOID_L, SIGMOID_X0, SIGMOID_K);
        self.index = self.lowpass(scaled);
        if self.sraw > 0.0 && self.sraw < 65000.0 {
            self.estimate(self.sraw, self.index);
            self.mox_std = self.variance.sqrt();
            self.mox_mean = self.mean + self.offset;
        }
        self.index
    }

    /// The learned baseline, to carry across restarts.
    pub fn state(&self) -> String {
        format!(
            "offset={}
mean={}
variance={}
uptime_gamma={}
uptime_gating={}
gating_duration={}
",
            self.offset, self.mean, self.variance, self.uptime_gamma, self.uptime_gating, self.gating_duration
        )
    }

    /// An engine that starts from a saved baseline: it still waits out the blackout, then carries
    /// on from what it had learned instead of taking the first value as normal air.
    pub fn restored(text: &str) -> Option<Voc> {
        let get = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('='))?.parse::<f32>().ok().filter(|v| v.is_finite());
        let mut v = Voc::new();
        v.offset = get("offset")?;
        v.mean = get("mean")?;
        v.variance = get("variance")?;
        v.uptime_gamma = get("uptime_gamma")?;
        v.uptime_gating = get("uptime_gating")?;
        v.gating_duration = get("gating_duration")?;
        v.initialized = true;
        v.mox_std = v.variance.sqrt();
        v.mox_mean = v.mean + v.offset;
        Some(v)
    }

    fn lowpass(&mut self, x: f32) -> f32 {
        if self.lp_fast == LP_UNSET || self.lp_slow == LP_UNSET || self.lp_out == LP_UNSET {
            self.lp_fast = x;
            self.lp_slow = x;
            self.lp_out = x;
        }
        let a_fast = SAMPLING / (SAMPLING + LP_TAU_FAST);
        let a_slow = SAMPLING / (SAMPLING + LP_TAU_SLOW);
        self.lp_fast = a_fast * x + self.lp_fast * (1.0 - a_fast);
        self.lp_slow = a_slow * x + self.lp_slow * (1.0 - a_slow);
        let e = ((self.lp_fast - self.lp_slow).abs() * LP_ALPHA).clamp(-20.0, 20.0).exp();
        let a = SAMPLING / (SAMPLING + LP_TAU_FAST + e * (LP_TAU_SLOW - LP_TAU_FAST));
        self.lp_out = x * a + self.lp_out * (1.0 - a);
        self.lp_out
    }

    fn estimate(&mut self, sraw: f32, index: f32) {
        if !self.initialized {
            self.initialized = true;
            self.mean = 0.0;
            self.offset = sraw;
            return;
        }
        if self.mean >= 100.0 || self.mean <= -100.0 {
            self.offset += self.mean;
            self.mean = 0.0;
        }
        self.gammas(index);
        let delta = (sraw - self.offset) - self.mean;
        self.variance = (1.0 - self.gamma_variance) * (self.variance + delta * self.gamma_variance * delta);
        self.mean += self.gamma_mean * delta;
    }

    fn gammas(&mut self, index: f32) {
        let gamma = SAMPLING / (SAMPLING + TAU_MEAN);
        let initial_mean = SAMPLING / (SAMPLING + TAU_INITIAL_MEAN);
        let initial_variance = SAMPLING / (SAMPLING + TAU_INITIAL_VARIANCE);
        self.uptime_gamma += SAMPLING;
        self.uptime_gating += SAMPLING;
        let s_mean = sigmoid(self.uptime_gamma, 1.0, INIT_DURATION_MEAN, INIT_TRANSITION);
        let threshold = GATING_THRESHOLD
            + sigmoid(self.uptime_gating, 1.0, INIT_DURATION_MEAN, INIT_TRANSITION) * (GATING_THRESHOLD_INITIAL - GATING_THRESHOLD);
        let gating_mean = sigmoid(index, 1.0, threshold, GATING_THRESHOLD_TRANSITION);
        self.gamma_mean = (gamma + s_mean * (initial_mean - gamma)) * gating_mean;
        let s_variance = sigmoid(self.uptime_gamma, 1.0, INIT_DURATION_VARIANCE, INIT_TRANSITION);
        let threshold = GATING_THRESHOLD
            + sigmoid(self.uptime_gating, 1.0, INIT_DURATION_VARIANCE, INIT_TRANSITION) * (GATING_THRESHOLD_INITIAL - GATING_THRESHOLD);
        let gating_variance = sigmoid(index, 1.0, threshold, GATING_THRESHOLD_TRANSITION);
        self.gamma_variance = (gamma + (s_variance - s_mean) * (initial_variance - gamma)) * gating_variance;
        let d = self.gating_duration + ((1.0 - gating_mean) * (1.0 + GATING_MAX_RATIO) - GATING_MAX_RATIO) * SAMPLING;
        self.gating_duration = d.max(0.0);
        if GATING_MAX_DURATION < self.gating_duration {
            self.uptime_gating = 0.0;
        }
    }
}

/// Seconds after power-on that the SGP40 is given at most to settle.
pub const SETTLE_MAX: f32 = 1800.0;
const SETTLE_RISE: f32 = 20.0;

/// After power-on the SGP40's raw signal climbs while the sensor heats up: for about 3 minutes
/// after a reboot, for half an hour after a night off. A restored baseline reads that as bad air,
/// so the engine waits until the last 5 minutes average under 20 counts above the 5 before them.
pub struct Warmup {
    uptime: f32,
    sum: f32,
    samples: u32,
    minutes: VecDeque<f32>,
}

impl Warmup {
    pub fn new(uptime: f32) -> Warmup {
        Warmup { uptime, sum: 0.0, samples: 0, minutes: VecDeque::new() }
    }

    pub fn uptime(&self) -> f32 {
        self.uptime
    }

    /// Takes one second's raw signal; true while it is still climbing.
    pub fn holding(&mut self, sraw: f32) -> bool {
        self.uptime += SAMPLING;
        self.sum += sraw;
        self.samples += 1;
        if self.samples == 60 {
            if self.minutes.len() == 10 {
                self.minutes.pop_front();
            }
            self.minutes.push_back(self.sum / 60.0);
            (self.sum, self.samples) = (0.0, 0);
        }
        if self.uptime >= SETTLE_MAX {
            return false;
        }
        if self.minutes.len() < 10 {
            return true;
        }
        let (before, last) = (self.minutes.range(..5).sum::<f32>(), self.minutes.range(5..).sum::<f32>());
        (last - before) / 5.0 >= SETTLE_RISE
    }
}

/// The app's mapping of the 0..500 index to the one it reports (TVOCAPI::updateValue).
pub fn reported(index: f32) -> i32 {
    let i = index.min(500.0).round() as f64;
    let mut x = ((501.0 - i).ln() - 6.24) * -2215.4 * 0.8;
    if x > 218.0 {
        x = (x / 200.0).sqrt() * 200.0;
    }
    let y = 501.0 - (x / -2215.4 + 6.24).exp();
    let r = if y < 0.0 { -((-y + 0.5).floor()) } else { (y + 0.5).floor() };
    r.clamp(0.0, 500.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_air_settles_near_100() {
        let mut v = Voc::new();
        let mut i = 0.0;
        for n in 0..20_000 {
            i = v.process(31_800.0 + (n % 7) as f32);
        }
        assert!((i - 100.0).abs() < 3.0, "{i}");
        assert_eq!(reported(100.0), 52);
    }

    #[test]
    fn restored_engine_keeps_its_baseline() {
        let mut v = Voc::new();
        for n in 0..20_000 {
            v.process(31_800.0 + (n % 7) as f32);
        }
        let mut restored = Voc::restored(&v.state()).unwrap();
        let mut fresh = Voc::new();
        let (mut r, mut f) = (0.0, 0.0);
        for _ in 0..300 {
            r = restored.process(30_500.0);
            f = fresh.process(30_500.0);
        }
        assert!(r > 200.0 && (f - 100.0).abs() < 5.0, "{r} {f}");
    }

    #[test]
    fn warmup_waits_for_the_raw_signal_to_level_off() {
        let settled = |minutes: &[f32]| {
            let mut w = Warmup::new(20.0);
            let mut n = 0;
            while w.holding(minutes[(n / 60).min(minutes.len() - 1)]) {
                n += 1;
            }
            w.uptime() as u32 / 60
        };
        let night_off = [
            30254.0, 30762.0, 31060.0, 31278.0, 31414.0, 31530.0, 31552.0, 31621.0, 31647.0, 31682.0, 31702.0, 31733.0, 31737.0,
            31786.0, 31759.0, 31788.0, 31793.0, 31790.0, 31775.0, 31806.0, 31829.0, 31815.0, 31819.0, 31875.0, 31833.0, 31852.0,
            31843.0, 31845.0, 31826.0, 31866.0, 31850.0,
        ];
        assert!((20..30).contains(&settled(&night_off)), "{}", settled(&night_off));
        assert_eq!(settled(&[31_800.0]), 10);
    }
}
