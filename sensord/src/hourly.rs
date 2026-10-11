//! Hourly summaries of the minute log for the long chart ranges: the mean, min and max of each
//! reading per hour, in a file per (UTC) year under /userdata/open-cgs2/hourly, kept for good. They
//! are built from sensord.csv and its rotated .1 once each hour has ended, so a restart loses
//! nothing; an hour without rows gets no row.
use crate::{log, CSV, MIN_TIME};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

const DIR: &str = "/userdata/open-cgs2/hourly";
const COLUMNS: [&str; 7] = ["temp", "humidity", "co2", "voc", "light", "pm25", "pm10"];
/// Hourly rows stop below this much free space, so the state files never run out of room.
const MIN_FREE: u64 = 20 << 20;

/// The hour up to which the minute log was last summarized, so each hour is read once.
static DONE: AtomicI64 = AtomicI64::new(0);
static LOW: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Agg {
    n: u32,
    sum: f64,
    min: f64,
    max: f64,
}

impl Default for Agg {
    fn default() -> Agg {
        Agg { n: 0, sum: 0.0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
}

impl Agg {
    fn add(&mut self, v: f64) {
        self.n += 1;
        self.sum += v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }
}

#[derive(Default)]
struct Hour {
    rows: u32,
    cols: [Agg; COLUMNS.len()],
}

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn header() -> String {
    let mut h = "time,rows".to_string();
    for c in COLUMNS {
        h += &format!(",{c},{c}_min,{c}_max");
    }
    h
}

fn line(start: i64, h: &Hour) -> String {
    let mut l = format!("{start},{}", h.rows);
    for a in &h.cols {
        if a.n == 0 {
            l += ",,,";
        } else {
            l += &format!(",{},{},{}", num(a.sum / a.n as f64), num(a.min), num(a.max));
        }
    }
    l
}

/// The UTC year of `t`.
fn year(t: i64) -> i64 {
    let z = t.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(mp >= 10)
}

/// Adds the minute rows in `[from, until)` of the log `text` to `hours`, by hour.
fn collect(text: impl BufRead, from: i64, until: i64, hours: &mut BTreeMap<i64, Hour>) {
    let mut lines = text.lines().map_while(Result::ok);
    let Some(head) = lines.next() else { return };
    let names: Vec<&str> = head.split(',').collect();
    let idx: Vec<Option<usize>> = COLUMNS.iter().map(|c| names.iter().position(|n| n == c)).collect();
    for l in lines {
        let fields: Vec<&str> = l.split(',').collect();
        let Some(t) = fields.first().and_then(|t| t.parse::<i64>().ok()) else { continue };
        if t < from || t >= until {
            continue;
        }
        let h = hours.entry(t.div_euclid(3600) * 3600).or_default();
        h.rows += 1;
        for (a, i) in h.cols.iter_mut().zip(&idx) {
            if let Some(v) = i.and_then(|i| fields.get(i)?.parse::<f64>().ok()).filter(|v| v.is_finite()) {
                a.add(v);
            }
        }
    }
}

/// The time of the first row of the log at `path`.
pub fn first_time(path: &str) -> Option<i64> {
    let mut lines = BufReader::new(File::open(path).ok()?).lines().map_while(Result::ok);
    lines.nth(1)?.split(',').next()?.parse().ok()
}

/// The start of the last hour written, from the newest year's file.
fn last_hour() -> Option<i64> {
    let newest = fs::read_dir(DIR)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".csv")?.parse::<i64>().ok())
        .max()?;
    let mut f = File::open(format!("{DIR}/{newest}.csv")).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(4096))).ok()?;
    let mut text = String::new();
    f.read_to_string(&mut text).ok()?;
    text.lines().filter_map(|l| l.split(',').next()?.parse::<i64>().ok()).last()
}

fn free_bytes(path: &str) -> Option<u64> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

fn write(hours: &BTreeMap<i64, Hour>) -> std::io::Result<()> {
    fs::create_dir_all(DIR)?;
    let mut by_year: BTreeMap<i64, String> = BTreeMap::new();
    for (start, h) in hours {
        let text = by_year.entry(year(*start)).or_default();
        text.push_str(&line(*start, h));
        text.push('\n');
    }
    for (y, text) in by_year {
        let path = format!("{DIR}/{y}.csv");
        let new = fs::metadata(&path).is_err();
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        if new {
            writeln!(f, "{}", header())?;
        }
        f.write_all(text.as_bytes())?;
    }
    Ok(())
}

/// Summarizes the hours that ended at least a minute ago and are not written yet.
pub fn update(now: i64) {
    let until = (now - 60).div_euclid(3600) * 3600;
    if now < MIN_TIME || DONE.load(Ordering::Relaxed) == until {
        return;
    }
    let low = free_bytes("/userdata").is_some_and(|b| b < MIN_FREE);
    if low != LOW.swap(low, Ordering::Relaxed) {
        log(if low { "hourly history paused: /userdata is nearly full" } else { "hourly history resumed" });
    }
    if low {
        return;
    }
    let from = last_hour().map_or(MIN_TIME, |h| (h + 3600).max(MIN_TIME));
    let mut hours = BTreeMap::new();
    if from < until {
        if first_time(CSV).is_none_or(|t| t > from) {
            if let Ok(f) = File::open(format!("{CSV}.1")) {
                collect(BufReader::new(f), from, until, &mut hours);
            }
        }
        if let Ok(f) = File::open(CSV) {
            collect(BufReader::new(f), from, until, &mut hours);
        }
    }
    match write(&hours) {
        Err(e) => log(format!("{DIR}: {e}")),
        Ok(()) if hours.len() > 1 => log(format!("hourly history: wrote {} hours", hours.len())),
        Ok(()) => {}
    }
    DONE.store(until, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_by_hour() {
        let log = "time,temp,humidity,co2,voc,light,pm25,pm10\n\
3540,20,50,800,60,3,,\n3600,21.5,40,900,,3,5,6\n3660,22.5,42,1100,70,4,,\n7200,23,45,1000,61,5,,\n";
        let mut hours = BTreeMap::new();
        collect(log.as_bytes(), 3600, 7200, &mut hours);
        let lines: Vec<String> = hours.iter().map(|(s, h)| line(*s, h)).collect();
        assert_eq!(lines, vec!["3600,2,22,21.5,22.5,41,40,42,1000,900,1100,70,70,70,3.5,3,4,5,5,5,6,6,6"]);
        assert_eq!(header().split(',').count(), lines[0].split(',').count());
    }

    #[test]
    fn years_change_at_utc_new_year() {
        assert_eq!(year(1767225599), 2025);
        assert_eq!(year(1767225600), 2026);
        assert_eq!(year(951782400), 2000);
    }
}
