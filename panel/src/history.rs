//! History for the sensor charts over a chosen range. This device's line comes from sensord's
//! logs (the minute log for 3 hours and a day, the hourly files beyond), the others from HA while
//! it is connected (its recorder for 3 hours, its statistics beyond). Longer ranges are drawn in
//! buckets aligned to local time, with this device's min and max as a band.
use crate::{ha, Chart, Shared};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CSV: &str = "/userdata/open-cgs2/sensord.csv";
const HOURLY: &str = "/userdata/open-cgs2/hourly";
/// Bytes per minute row in sensord's log, with room to spare.
const ROW: u64 = 200;
/// sensord logs a row a minute; an older row means it was not running, which shows as a gap.
const LOCAL_MAX_AGE: f64 = 150.0;
const HA_WAIT: Duration = Duration::from_secs(10);
const DAY: i64 = 86400;
const WEEK: i64 = 7 * DAY;
/// Chart keys and their columns in sensord's logs.
const COLUMNS: [(&str, &str); 6] = [("temp", "temp"), ("rh", "humidity"), ("co2", "co2"), ("tvoc", "voc"), ("pm25", "pm25"), ("pm10", "pm10")];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Range {
    Hours3,
    Day,
    Week,
    Month,
    Year,
    All,
}

impl Range {
    fn parse(s: &str) -> Option<Range> {
        Some(match s {
            "3h" => Range::Hours3,
            "1d" => Range::Day,
            "1w" => Range::Week,
            "1m" => Range::Month,
            "1y" => Range::Year,
            "all" => Range::All,
            _ => return None,
        })
    }
}

/// `count` buckets of `step` seconds, the first starting at `first` in local time (seconds since
/// the epoch as if the local clock were UTC).
#[derive(Debug, PartialEq)]
struct Grid {
    first: i64,
    step: i64,
    count: usize,
}

impl Grid {
    fn end(&self) -> i64 {
        self.first + (self.count as i64 - 1) * self.step
    }
}

/// A value over a stretch of time: a reading, an hour of them or one of HA's statistics.
#[derive(Clone, Copy, Debug)]
struct Point {
    t: i64,
    mean: f64,
    lo: f64,
    hi: f64,
    weight: f64,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Seconds east of UTC at `t`, from the device's time zone.
pub fn offset(t: i64) -> i64 {
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&(t as libc::c_long), &mut tm).is_null() {
            0
        } else {
            tm.tm_gmtoff as i64
        }
    }
}

fn local(t: i64) -> i64 {
    t + offset(t)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn iso(secs: i64) -> String {
    let (y, m, d) = civil(secs.div_euclid(DAY));
    let t = secs.rem_euclid(DAY);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00", t / 3600, t / 60 % 60, t % 60)
}

/// The start of the bucket holding local time `t`: weeks start on Monday, the rest at midnight.
fn align(t: i64, step: i64) -> i64 {
    if step == WEEK {
        let d = t.div_euclid(DAY);
        (d - (d + 3).rem_euclid(7)) * DAY
    } else {
        t.div_euclid(step) * step
    }
}

fn grid(range: Range, now: i64, earliest: Option<i64>) -> Grid {
    let (span, step) = match range {
        Range::Hours3 => (3 * 3600, 60),
        Range::Day => (DAY, 600),
        Range::Week => (WEEK, 3600),
        Range::Month => (30 * DAY, 4 * 3600),
        Range::Year => (365 * DAY, DAY),
        Range::All => {
            let span = now - earliest.unwrap_or(now);
            let step = match span {
                s if s <= 10 * DAY => 3600,
                s if s <= 60 * DAY => 4 * 3600,
                s if s <= 400 * DAY => DAY,
                _ => WEEK,
            };
            (span, step)
        }
    };
    let last = align(local(now), step);
    let first = match (range, earliest) {
        (Range::All, Some(e)) => align(local(e), step).min(last),
        _ => last - span / step * step,
    };
    Grid { first, step, count: ((last - first) / step + 1) as usize }
}

/// Averages `points` into the grid's buckets, weighted, with the lowest and highest of each.
fn bucket(points: &[Point], g: &Grid) -> (Vec<Option<f64>>, Vec<Option<f64>>, Vec<Option<f64>>) {
    let mut acc = vec![(0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY); g.count];
    for p in points {
        let i = (local(p.t) - g.first).div_euclid(g.step);
        if let Some(a) = usize::try_from(i).ok().and_then(|i| acc.get_mut(i)) {
            a.0 += p.mean * p.weight;
            a.1 += p.weight;
            a.2 = a.2.min(p.lo);
            a.3 = a.3.max(p.hi);
        }
    }
    let round = |v: f64| (v * 100.0).round() / 100.0;
    let pick = |f: fn(&(f64, f64, f64, f64)) -> f64| -> Vec<Option<f64>> { acc.iter().map(|a| (a.1 > 0.0).then(|| round(f(a)))).collect() };
    (pick(|a| a.0 / a.1), pick(|a| a.2), pick(|a| a.3))
}

/// Value of each minute from `start` on, carrying the last one forward for at most `max_age` seconds.
fn sample(points: &[(f64, Option<f64>)], start: i64, count: usize, max_age: Option<f64>) -> Vec<Option<f64>> {
    let mut out = Vec::with_capacity(count);
    let mut i = 0;
    let mut cur = None;
    for p in 0..count {
        let t = (start + p as i64 * 60) as f64;
        while let Some(&(at, v)) = points.get(i).filter(|(at, _)| *at <= t) {
            cur = Some((at, v));
            i += 1;
        }
        out.push(cur.filter(|(at, _)| max_age.is_none_or(|m| t - at <= m)).and_then(|(_, v)| v));
    }
    out
}

/// HA's minimal history for one entity as points.
fn ha_states(changes: &Value) -> Vec<(f64, Option<f64>)> {
    changes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["lu"].as_f64()?, c["s"].as_str().and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite()))))
        .collect()
}

/// HA's statistics for one entity as points.
fn ha_statistics(rows: &Value) -> Vec<Point> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let mean = r["mean"].as_f64()?;
            Some(Point {
                t: r["start"].as_i64()? / 1000,
                mean,
                lo: r["min"].as_f64().unwrap_or(mean),
                hi: r["max"].as_f64().unwrap_or(mean),
                weight: 1.0,
            })
        })
        .collect()
}

/// The rows of `text` (a CSV log, or the end of one) as the time and the `col`th value.
fn parse_rows(text: &str, col: usize) -> Vec<(f64, Option<f64>)> {
    text.lines()
        .filter_map(|l| {
            let t = l.split(',').next()?.parse::<f64>().ok()?;
            Some((t, l.split(',').nth(col).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite())))
        })
        .collect()
}

/// `name` from the last `tail` bytes of the log at `path`, and whether that was the whole file.
fn read_log(path: &str, name: &str, tail: u64) -> Option<(Vec<(f64, Option<f64>)>, bool)> {
    let mut f = File::open(path).ok()?;
    let mut header = String::new();
    BufReader::new(&f).read_line(&mut header).ok()?;
    let col = header.trim_end().split(',').position(|c| c == name)?;
    let from = f.metadata().ok()?.len().saturating_sub(tail);
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    Some((parse_rows(text, col), from == 0))
}

/// This device's `name` from the minute log since `start`, reaching into the rotated log.
fn minutes(name: &str, start: i64, now: i64) -> Vec<(f64, Option<f64>)> {
    let tail = ((now - start).max(0) as u64 / 60 + 60) * ROW;
    let (mut points, whole) = read_log(CSV, name, tail).unwrap_or_default();
    if whole && points.first().is_none_or(|(t, _)| *t > start as f64) {
        let (mut older, _) = read_log(&format!("{CSV}.1"), name, tail).unwrap_or_default();
        older.append(&mut points);
        points = older;
    }
    points.retain(|(t, _)| *t >= start as f64);
    points
}

fn hourly_years() -> Vec<i64> {
    let mut years: Vec<i64> = std::fs::read_dir(HOURLY)
        .map(|d| d.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".csv")?.parse().ok()).collect())
        .unwrap_or_default();
    years.sort_unstable();
    years
}

/// This device's `name` from the hourly files since `start`, weighted by each hour's rows.
fn hours(name: &str, start: i64) -> Vec<Point> {
    let first_year = civil(start.div_euclid(DAY)).0;
    let mut out = Vec::new();
    for y in hourly_years().into_iter().filter(|y| *y >= first_year) {
        let Ok(f) = File::open(format!("{HOURLY}/{y}.csv")) else { continue };
        let mut lines = BufReader::new(f).lines().map_while(Result::ok);
        let Some(head) = lines.next() else { continue };
        let cols: Vec<&str> = head.split(',').collect();
        let at = |c: &str| cols.iter().position(|n| *n == c);
        let (Some(mean), Some(lo), Some(hi), Some(rows)) = (at(name), at(&format!("{name}_min")), at(&format!("{name}_max")), at("rows")) else { continue };
        for l in lines {
            let f: Vec<&str> = l.split(',').collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite());
            let Some(t) = f.first().and_then(|t| t.parse::<i64>().ok()).filter(|t| *t >= start) else { continue };
            if let Some(m) = num(mean) {
                out.push(Point { t, mean: m, lo: num(lo).unwrap_or(m), hi: num(hi).unwrap_or(m), weight: num(rows).unwrap_or(1.0) });
            }
        }
    }
    out
}

/// The first time sensord has history for, from the oldest hourly file or else the minute logs.
fn earliest() -> Option<i64> {
    let oldest = hourly_years().into_iter().next().and_then(|y| {
        let mut lines = BufReader::new(File::open(format!("{HOURLY}/{y}.csv")).ok()?).lines().map_while(Result::ok);
        lines.nth(1)?.split(',').next()?.parse::<i64>().ok()
    });
    oldest.or_else(|| {
        [format!("{CSV}.1"), CSV.to_string()].iter().find_map(|p| {
            let mut lines = BufReader::new(File::open(p).ok()?).lines().map_while(Result::ok);
            lines.nth(1)?.split(',').next()?.parse::<i64>().ok()
        })
    })
}

/// This device's `name` as points since `start`: hours from the hourly files, then the minutes
/// after the last hour written.
fn local_points(name: &str, start: i64, now: i64, hourly: bool) -> Vec<Point> {
    let mut points = if hourly { hours(name, start) } else { Vec::new() };
    let after = points.last().map_or(start, |p| p.t + 3600);
    points.extend(minutes(name, after, now).into_iter().filter_map(|(t, v)| {
        v.map(|v| Point { t: t as i64, mean: v, lo: v, hi: v, weight: 1.0 })
    }));
    points
}

fn ticks(g: &Grid) -> Vec<Value> {
    let span = g.end() - g.first;
    let mut out = Vec::new();
    let mut push = |t: i64, label: String| {
        if t >= g.first && t <= g.end() {
            out.push(json!({"i": (t - g.first) as f64 / g.step as f64, "label": label}));
        }
    };
    let every = |n: i64| (g.first.div_euclid(n) + 1) * n;
    if span <= 2 * DAY {
        let n = if span <= 6 * 3600 { 3600 } else { 6 * 3600 };
        for t in (every(n)..=g.end()).step_by(n as usize) {
            push(t, format!("{:02}:00", t.rem_euclid(DAY) / 3600));
        }
    } else if span <= 10 * DAY {
        for t in (every(DAY)..=g.end()).step_by(DAY as usize) {
            push(t, WEEKDAYS[(t / DAY + 4).rem_euclid(7) as usize].into());
        }
    } else if span <= 70 * DAY {
        for t in (align(g.first, WEEK) + WEEK..=g.end()).step_by(WEEK as usize) {
            let (_, m, d) = civil(t / DAY);
            push(t, format!("{} {d}", MONTHS[m as usize - 1]));
        }
    } else {
        let (mut y, mut m, _) = civil(g.first.div_euclid(DAY));
        let yearly = span > 800 * DAY;
        loop {
            (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            let t = days_from_civil(y, m, 1) * DAY;
            if t > g.end() {
                break;
            }
            if m == 1 {
                push(t, y.to_string());
            } else if !yearly {
                push(t, MONTHS[m as usize - 1].into());
            }
        }
    }
    out
}

fn local_column(chart: &Chart, here: bool) -> Option<&'static str> {
    COLUMNS.iter().find(|(k, _)| here && *k == chart.key).map(|(_, c)| *c)
}

fn ask(shared: &Shared, tx: &Sender<ha::Cmd>, msg: Value) -> Value {
    if !shared.read(|s| s.connected) {
        return Value::Null;
    }
    let (reply, result) = mpsc::channel();
    if tx.send(ha::Cmd::Request(msg, reply)).is_err() {
        return Value::Null;
    }
    result.recv_timeout(HA_WAIT).unwrap_or_default()
}

pub fn chart(shared: &Shared, tx: &Sender<ha::Cmd>, key: &str, range: &str) -> Option<Value> {
    let chart = shared.cfg.charts.iter().find(|c| c.key == key)?;
    let range = Range::parse(range)?;
    let now = now();
    let g = grid(range, now, if range == Range::All { earliest() } else { None });
    let start = g.first - offset(now) - 3600;
    let entities: Vec<String> =
        chart.series.iter().filter(|s| local_column(chart, s.here).is_none()).filter_map(|s| s.entity.clone()).collect();
    let recorded = match (entities.is_empty(), range) {
        (true, _) => Value::Null,
        (false, Range::Hours3) => ask(
            shared,
            tx,
            json!({"type": "history/history_during_period", "start_time": iso(start), "entity_ids": entities,
                "minimal_response": true, "no_attributes": true, "significant_changes_only": false}),
        ),
        (false, _) => {
            let period = match g.step {
                600 => "5minute",
                DAY => "day",
                WEEK => "week",
                _ => "hour",
            };
            ask(
                shared,
                tx,
                json!({"type": "recorder/statistics_during_period", "start_time": iso(start), "period": period,
                    "statistic_ids": entities, "types": ["mean", "min", "max"]}),
            )
        }
    };
    let utc_start = now / 60 * 60 - 3 * 3600;
    let series: Vec<Value> = chart
        .series
        .iter()
        .map(|s| {
            let mut v = json!({"name": s.name, "here": s.here, "outside": s.outside});
            match (range, local_column(chart, s.here), &s.entity) {
                (Range::Hours3, Some(col), _) => v["values"] = json!(sample(&minutes(col, utc_start, now), utc_start, g.count, Some(LOCAL_MAX_AGE))),
                (Range::Hours3, None, Some(e)) => v["values"] = json!(sample(&ha_states(&recorded[e]), utc_start, g.count, None)),
                (_, Some(col), _) => {
                    let (values, lo, hi) = bucket(&local_points(col, start, now, range != Range::Day), &g);
                    (v["values"], v["lo"], v["hi"]) = (json!(values), json!(lo), json!(hi));
                }
                (_, None, Some(e)) => v["values"] = json!(bucket(&ha_statistics(&recorded[e]), &g).0),
                (_, None, None) => v["values"] = json!(vec![Value::Null; g.count]),
            }
            if key == "temp" && s.here && shared.cfg.fahrenheit {
                for k in ["values", "lo", "hi"] {
                    if let Some(a) = v[k].as_array_mut() {
                        a.iter_mut().filter_map(|x| Some((x.as_f64()?, x))).for_each(|(c, x)| *x = json!(crate::local::fahrenheit(c)));
                    }
                }
            }
            v
        })
        .collect();
    Some(json!({"start": g.first, "step": g.step, "series": series, "ticks": ticks(&g)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() {
        static TZ: std::sync::Once = std::sync::Once::new();
        TZ.call_once(|| std::env::set_var("TZ", "UTC"));
    }

    #[test]
    fn formats_utc() {
        assert_eq!(iso(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(iso(1791409785), "2026-10-07T21:49:45+00:00");
        assert_eq!(iso(951782400), "2000-02-29T00:00:00+00:00");
        assert_eq!(days_from_civil(2026, 10, 7), 1791409785 / DAY);
    }

    #[test]
    fn carries_states_forward() {
        let changes = json!([{"s": "10", "lu": 50.0}, {"s": "unavailable", "lu": 130.0}, {"s": "12.5", "lu": 200.0}]);
        assert_eq!(sample(&ha_states(&changes), 0, 5, None), vec![None, Some(10.0), Some(10.0), None, Some(12.5)]);
    }

    #[test]
    fn leaves_gaps_where_the_log_stops() {
        let log = "time,temp,humidity\n0,21.5,40\n60,21.6,\n120,21.7,41\n400,22,42\n";
        let temp = parse_rows(log, 1);
        assert_eq!(sample(&temp, 0, 8, Some(LOCAL_MAX_AGE)), vec![Some(21.5), Some(21.6), Some(21.7), Some(21.7), Some(21.7), None, None, Some(22.0)]);
        assert_eq!(sample(&parse_rows(log, 2), 0, 3, Some(LOCAL_MAX_AGE)), vec![Some(40.0), None, Some(41.0)]);
    }

    #[test]
    fn aligns_buckets() {
        utc();
        let now = 1791748800 + 1234;
        assert_eq!(align(now, WEEK) / DAY % 7, 4, "weeks start on Monday, 4 days after a Thursday");
        assert_eq!(grid(Range::Day, now, None), Grid { first: 1791748800 + 1200 - DAY, step: 600, count: 145 });
        let all = grid(Range::All, now, Some(now - 100 * DAY));
        assert_eq!((all.step, all.count), (DAY, 101));
        assert_eq!(grid(Range::All, now, Some(now - 2 * DAY)).step, 3600);
        assert_eq!(grid(Range::All, now, Some(now - 500 * DAY)).step, WEEK);
    }

    #[test]
    fn buckets_weighted_with_extremes() {
        utc();
        let g = Grid { first: 0, step: 3600, count: 3 };
        let p = |t, mean, lo, hi, weight| Point { t, mean, lo, hi, weight };
        let (values, lo, hi) = bucket(&[p(0, 20.0, 19.0, 21.0, 60.0), p(3600, 22.0, 22.0, 22.0, 1.0), p(3660, 25.0, 25.0, 25.0, 3.0)], &g);
        assert_eq!(values, vec![Some(20.0), Some(24.25), None]);
        assert_eq!((lo[1], hi[1], hi[0]), (Some(22.0), Some(25.0), Some(21.0)));
    }

    #[test]
    fn labels_ticks_by_span() {
        utc();
        let day = days_from_civil(2026, 10, 11) * DAY;
        let labels = |g: Grid| ticks(&g).iter().map(|t| t["label"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(labels(Grid { first: day + 3600 * 9 + 60, step: 60, count: 181 }), ["10:00", "11:00", "12:00"]);
        assert_eq!(labels(Grid { first: day - 6 * DAY, step: 3600, count: 7 * 24 }), ["Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
        assert_eq!(labels(Grid { first: day - 364 * DAY, step: DAY, count: 365 })[..4], ["Nov", "Dec", "2026", "Feb"]);
    }
}
