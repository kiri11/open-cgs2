//! The stock app's compensation models (`/qingping/etc/models.ini`, TempFixPart in QingSnow2App).
//! A part is a piecewise polynomial of one input, split at its `pos` values, in thousandths of a
//! degree. Parsing follows the app, including C's lenient number parsing of `[0.0`.

#[derive(Default)]
pub struct Part {
    pos: Vec<i32>,
    pow: Vec<i64>,
    weights: Vec<Vec<f64>>,
    offset: Vec<f64>,
}

impl Part {
    pub fn load(text: &str, name: &str) -> Option<Part> {
        let mut part = Part::default();
        let mut inside = false;
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let kv: Vec<&str> = line.split('=').collect();
            let [k, v] = kv[..] else { return None };
            if !inside && !(k == "model" && v == name) {
                continue;
            }
            if k == "model" && v != name {
                break;
            }
            match k {
                "pos" => part.pos.push(atoi(v)),
                "pow" => part.pow.push(atoi(v) as i64),
                "weights" => {
                    let w: Vec<f64> = v.split(',').map(atof).collect();
                    if part.pow.last().map_or(true, |&p| p + 1 != w.len() as i64) {
                        return None;
                    }
                    part.weights.push(w);
                }
                "offset" => part.offset.push(atof(v)),
                _ => {}
            }
            inside = true;
        }
        (!part.pow.is_empty()).then_some(part)
    }

    fn calculate(&self, x: i32, seg: usize) -> f32 {
        let mut sum = 0.0;
        for i in 0..=self.pow.get(seg).copied().unwrap_or(0).max(0) as usize {
            let w = self.weights.get(seg).and_then(|w| w.get(i)).copied().unwrap_or(0.0);
            sum += if i == 0 { w } else { w * (x as f64 * 0.001).powf(i as f64) * 1000f64.powf(i as f64) };
        }
        sum += self.offset.get(seg).copied().unwrap_or(0.0);
        if sum.abs() > 0.0 {
            sum /= 1000.0;
        }
        sum as f32
    }

    pub fn adjust(&self, x: i32) -> f32 {
        match self.pos.iter().position(|&p| x <= p) {
            Some(i) => self.calculate(x, i),
            None => self.pos.last().map_or(0.0, |&p| self.calculate(p, self.pos.len() - 1)),
        }
    }

    pub fn max_pos(&self) -> i32 {
        self.pos.iter().copied().max().unwrap_or(0)
    }
}

fn number_prefix(s: &str, float: bool) -> &str {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    let mut any = digits(&mut i);
    if float && i < b.len() && b[i] == b'.' {
        i += 1;
        any |= digits(&mut i);
    }
    if !any {
        return "";
    }
    if float && i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) {
            i = j;
        }
    }
    &s[..i]
}

pub fn atof(s: &str) -> f64 {
    number_prefix(s, true).parse().unwrap_or(0.0)
}

pub fn atoi(s: &str) -> i32 {
    number_prefix(s, false).parse().unwrap_or(0)
}

#[cfg(test)]
pub const STOCK: &str = "model=preheating
pos=3300
pow=1
weights=[0.0, -0.45930429]
offset=-0.18001980593342637

model=charging
pos=1000
pow=1
weights=[0.0, -1.2]
offset=0

model=charging
pos=4000
pow=1
weights=[0.0, -0.4]
offset=-620

model=chargingdone
pos=4000
pow=1
weights=[0.0, 0]
offset=0


model=discharging
pos=3600
pow=1
weights=[0.0, 0.0]
offset=0.0


model=bright
pos=1000
pow=1
weights=[0, -3.150909090909091]
offset=-2.7272727

model=wifi
pos=1
pow=1
weights=[0.0, 0.0]
offset=0

model=art
pos=1
pow=1
weights=[0.0, 0.0]
offset=0
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charging_is_piecewise_and_clamped() {
        let p = Part::load(STOCK, "charging").unwrap();
        assert!((p.adjust(500) + 0.6).abs() < 1e-6);
        assert!((p.adjust(2000) + 1.42).abs() < 1e-6);
        assert!((p.adjust(9000) + 2.22).abs() < 1e-6);
        assert_eq!(p.max_pos(), 4000);
    }

    #[test]
    fn missing_model_is_none() {
        assert!(Part::load(STOCK, "sleep").is_none());
    }
}
