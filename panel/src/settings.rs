//! The Settings screen's backend: device details, Wi-Fi through wpa_supplicant, sensor
//! calibration (applied by sensord) and a one-off boot into the stock firmware.
use crate::{log, Config};
use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

const WPA_CONF: &str = "/data/cfg/wpa_supplicant.conf";
const SENSORS: &str = "/run/open-cgs2/sensors";
const CO2_CMD: &str = "/run/open-cgs2/co2/cmd";
const CO2_STATE: &str = "/userdata/open-cgs2/co2.state";
const STOCK_ONCE: &str = "/userdata/open-cgs2/stock-once";
const OS_RELEASE: &str = "/qingping/etc/os-release";
const CALIBRATION: &str = "/userdata/open-cgs2/calibration.json";
/// What each calibration value may be, and its default.
const ADJUST: [(&str, f64, f64, f64); 3] = [("humidity_scale", 0.5, 1.5, 0.9), ("temperature_offset", -10.0, 10.0, 0.0), ("humidity_offset", -20.0, 20.0, 0.0)];
const VERSION: &str = match option_env!("OPEN_CGS2_VERSION") {
    Some(v) => v,
    None => "dev",
};

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// `key=value` lines, as wpa_cli and the state files write them.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

fn wpa_cli(args: &[&str]) -> String {
    match Command::new("wpa_cli").arg("-i").arg("wlan0").args(args).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(e) => {
            log(format!("wpa_cli: {e}"));
            String::new()
        }
    }
}

/// wpa_cli prints SSIDs with `\xNN`, `\"` and `\\` escapes.
fn unescape(s: &str) -> Vec<u8> {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'x' && i + 3 < b.len() {
                if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                    out.push(v);
                    i += 4;
                    continue;
                }
            }
            out.push(b[i + 1]);
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn uptime() -> u64 {
    read("/proc/uptime").split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// The calibration in effect: what Settings saved, else config.json's, else the defaults.
fn calibration(cfg: &Config) -> Value {
    let saved: Value = serde_json::from_str(&read(CALIBRATION)).unwrap_or_default();
    ADJUST.iter().map(|(k, _, _, d)| (k.to_string(), json!(saved[k].as_f64().or(cfg.calibration[k].as_f64()).unwrap_or(*d)))).collect()
}

/// Saves the calibration for sensord, which picks it up within 10 s.
pub fn set_calibration(v: &Value) -> bool {
    let mut out = serde_json::Map::new();
    for (k, low, high, _) in ADJUST {
        match v[k].as_f64() {
            Some(x) if (low..=high).contains(&x) => drop(out.insert(k.into(), json!(x))),
            None => {}
            Some(_) => return false,
        }
    }
    let tmp = format!("{CALIBRATION}.tmp");
    let res = fs::write(&tmp, Value::Object(out).to_string()).and_then(|_| fs::rename(&tmp, CALIBRATION));
    if let Err(e) = &res {
        log(format!("{CALIBRATION}: {e}"));
    }
    res.is_ok()
}

pub fn info(cfg: &Config) -> Value {
    let status = wpa_cli(&["status"]);
    let rssi: Option<i64> = field(&wpa_cli(&["signal_poll"]), "RSSI").and_then(|v| v.parse().ok());
    let connected = field(&status, "wpa_state") == Some("COMPLETED");
    let sensors: Value = serde_json::from_str(&read(SENSORS)).unwrap_or_default();
    let co2 = read(CO2_STATE);
    json!({
        "name": cfg.name,
        "version": VERSION,
        "adjust": calibration(cfg),
        "wifi": {
            "connected": connected,
            "ssid": field(&status, "ssid").filter(|_| connected).map(|s| String::from_utf8_lossy(&unescape(s)).into_owned()),
            "ip": field(&status, "ip_address"),
            "rssi": rssi.filter(|_| connected),
        },
        "mac": read("/sys/class/net/wlan0/address").trim(),
        "battery": sensors["battery"],
        "charging": sensors["charging"],
        "co2": sensors["co2"],
        "uptime": uptime(),
        "firmware": field(&read(OS_RELEASE), "CLEARGRASS_VERSION"),
        "calibration": {
            "time": field(&co2, "time").and_then(|v| v.parse::<i64>().ok()),
            "ppm": field(&co2, "ppm").and_then(|v| v.parse::<i64>().ok()),
            "correction": field(&co2, "correction").and_then(|v| v.parse::<i64>().ok()),
            "error": field(&co2, "error"),
        },
    })
}

/// Networks in range, strongest first, one entry per name.
pub fn scan() -> Value {
    wpa_cli(&["scan"]);
    thread::sleep(Duration::from_secs(4));
    let mut seen: Vec<(String, i64, bool)> = Vec::new();
    for line in wpa_cli(&["scan_results"]).lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [_, _, level, flags, ssid] = cols[..] else { continue };
        let ssid = String::from_utf8_lossy(&unescape(ssid)).into_owned();
        let level: i64 = level.parse().unwrap_or(-100);
        if ssid.is_empty() || ssid.contains('\0') {
            continue;
        }
        let secured = flags.contains("WPA") || flags.contains("WEP") || flags.contains("SAE");
        match seen.iter_mut().find(|n| n.0 == ssid) {
            Some(n) => n.1 = n.1.max(level),
            None => seen.push((ssid, level, secured)),
        }
    }
    seen.sort_by(|a, b| b.1.cmp(&a.1));
    json!(seen.iter().map(|(ssid, rssi, secured)| json!({"ssid": ssid, "rssi": rssi, "secured": secured})).collect::<Vec<_>>())
}

/// The network blocks of wpa_supplicant.conf, and the lines before them.
fn blocks(conf: &str) -> (String, Vec<String>) {
    let (mut head, mut blocks, mut cur): (String, Vec<String>, Option<String>) = (String::new(), Vec::new(), None);
    for line in conf.lines() {
        match &mut cur {
            Some(b) => {
                b.push_str(line);
                b.push('\n');
                if line.trim() == "}" {
                    blocks.extend(cur.take());
                }
            }
            None if line.trim_start().starts_with("network={") => cur = Some(format!("{line}\n")),
            None => {
                head.push_str(line);
                head.push('\n');
            }
        }
    }
    (head.trim_end().to_string(), blocks)
}

/// The SSID a network block is for, as bytes.
fn block_ssid(block: &str) -> Option<Vec<u8>> {
    let v = block.lines().find_map(|l| l.trim().strip_prefix("ssid="))?;
    match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(quoted) => Some(quoted.as_bytes().to_vec()),
        None => (0..v.len()).step_by(2).map(|i| u8::from_str_radix(v.get(i..i + 2)?, 16).ok()).collect(),
    }
}

/// Adds the network to wpa_supplicant.conf ahead of the others (replacing an entry with the same
/// name) and reloads it. The old networks stay, so a wrong password falls back to them.
pub fn connect(ssid: &str, password: &str) -> Result<(), String> {
    let key = if password.is_empty() {
        "\tkey_mgmt=NONE\n".to_string()
    } else {
        if !(8..=63).contains(&password.len()) {
            return Err("The password must be 8 to 63 characters".into());
        }
        let out = Command::new("wpa_passphrase").arg(ssid).arg(password).output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout);
        let psk = text.lines().find_map(|l| l.trim().strip_prefix("psk=")).filter(|p| p.len() == 64).ok_or("wpa_passphrase failed")?;
        format!("\tpsk={psk}\n\tkey_mgmt=WPA-PSK\n\tieee80211w=1\n")
    };
    let (head, mut blocks) = blocks(&read(WPA_CONF));
    blocks.retain(|b| block_ssid(b).as_deref() != Some(ssid.as_bytes()));
    let priority = blocks.iter().filter_map(|b| b.lines().find_map(|l| l.trim().strip_prefix("priority=")?.parse::<i64>().ok())).max().unwrap_or(0) + 1;
    blocks.insert(0, format!("network={{\n\tssid={}\n\tscan_ssid=1\n{key}\tpriority={priority}\n}}\n", hex(ssid.as_bytes())));
    let text = format!("{head}\n\n{}", blocks.join("\n"));
    let tmp = format!("{WPA_CONF}.tmp");
    fs::write(&tmp, text).and_then(|_| fs::rename(&tmp, WPA_CONF)).map_err(|e| e.to_string())?;
    let reply = wpa_cli(&["reconfigure"]);
    log(format!("wifi: added {ssid:?}, reconfigure: {}", reply.trim()));
    Ok(())
}

pub fn calibrate(ppm: u16) -> bool {
    let res = fs::create_dir_all("/run/open-cgs2/co2").and_then(|_| fs::write(CO2_CMD, format!("calibrate {ppm}\n")));
    if let Err(e) = &res {
        log(format!("{CO2_CMD}: {e}"));
    }
    res.is_ok()
}

/// Reboots into the stock firmware once; the launcher removes the file and the boot after that
/// is an open-cgs2 one again.
pub fn stock_once() -> bool {
    match fs::write(STOCK_ONCE, "") {
        Ok(()) => {
            log("restarting into the stock firmware once");
            Command::new("sh").args(["-c", "sync; reboot"]).spawn().is_ok()
        }
        Err(e) => {
            log(format!("{STOCK_ONCE}: {e}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_other_networks_when_parsing() {
        let conf = "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\n\nnetwork={\n\tssid=\"Home\"\n\tpsk=\"x\"\n}\nnetwork={\n\tssid=4361666520e282ac\n\tkey_mgmt=NONE\n}\n";
        let (head, b) = blocks(conf);
        assert_eq!(head, "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1");
        assert_eq!(b.len(), 2);
        assert_eq!(block_ssid(&b[0]).unwrap(), b"Home");
        assert_eq!(block_ssid(&b[1]).unwrap(), "Cafe €".as_bytes());
        assert_eq!(unescape(r#"Caf\xc3\xa9 \"x\""#), "Café \"x\"".as_bytes());
    }
}
