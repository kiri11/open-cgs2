//! Installs, updates and removes open-cgs2 on Qingping CGS2 air monitors over ADB. Run without
//! arguments for a step-by-step setup; see `cgs2 help` for the commands.
mod device;
mod key;
mod wizard;

use device::Device;
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

const VERSION: &str = match option_env!("OPEN_CGS2_VERSION") {
    Some(v) => v,
    None => "dev",
};
const BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundle.tar"));
const STATE: &str = "/userdata/open-cgs2";
const HELP: &str = "cgs2: install open-cgs2 on Qingping CGS2 air monitors over ADB (port 5555)

Usage:
  cgs2                         step-by-step setup
  cgs2 status HOST             mode, firmware layout and installed version
  cgs2 install HOST [options]  install, or reinstall keeping the device's settings
      --config FILE              use this config.json
      --lan-only                 no default route and no IPv6: the device only talks to the LAN
      --open-adb                 leave ADB open to anyone instead of only this PC's key
      --ssh-key FILE             allow this public key to log in over SSH as root
      --bundle DIR               device files from DIR instead of the ones built in
  cgs2 update HOST [--bundle DIR]  new device files, settings kept
  cgs2 config HOST [FILE]      change settings, or push FILE as config.json
  cgs2 logs HOST [NAME]        panel, sensord, screen, supervisor or boot (default panel)
  cgs2 uninstall HOST [--purge]  restore the stock firmware; --purge also removes settings and history
";

/// The device files: paths in the bundle and their contents.
pub struct Bundle(Vec<(String, Vec<u8>)>);

impl Bundle {
    pub fn load(dir: Option<&str>) -> Result<Bundle, String> {
        let mut files = Vec::new();
        match dir {
            Some(d) => walk(Path::new(d), Path::new(d), &mut files)?,
            None if BUNDLE.is_empty() => return Err("this build has no device files; pass --bundle DIR".into()),
            None => {
                let mut tar = tar::Archive::new(BUNDLE);
                for entry in tar.entries().map_err(|e| e.to_string())? {
                    let mut entry = entry.map_err(|e| e.to_string())?;
                    if entry.header().entry_type().is_file() {
                        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().trim_start_matches("./").to_string();
                        let mut data = Vec::new();
                        entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
                        files.push((path, data));
                    }
                }
            }
        }
        Ok(Bundle(files))
    }

    fn get(&self, path: &str) -> Option<&[u8]> {
        self.0.iter().find(|(p, _)| p == path).map(|(_, d)| d.as_slice())
    }

    /// The components shipped, with their manifests.
    pub fn components(&self) -> Vec<(String, Value)> {
        let mut out: Vec<(String, Value)> = self
            .0
            .iter()
            .filter_map(|(p, d)| {
                let name = p.strip_prefix("open-cgs2/ui/components/")?.strip_suffix("/component.json")?;
                Some((name.to_string(), serde_json::from_slice(d).ok()?))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, std::fs::read(&path).map_err(|e| e.to_string())?));
        }
    }
    Ok(())
}

pub struct Options {
    pub config: Option<Value>,
    pub lan_only: bool,
    pub open_adb: bool,
    pub ssh_key: Option<String>,
}

/// Where the device keeps open-cgs2: /opt on a writable root, else /oem, mounted over /opt at boot.
pub struct Layout {
    pub read_only: bool,
}

impl Layout {
    pub fn detect(d: &mut Device) -> Result<Layout, String> {
        if !d.test("[ -e /qingping/bin/start.sh ] || [ -e /oem/bin/start-stock.sh ]") {
            return Err(format!("{} does not look like a Qingping CGS2", d.host));
        }
        Ok(Layout { read_only: d.test("awk '$2 == \"/\" && ($3 == \"squashfs\" || $4 ~ /^ro/) { found = 1 } END { exit !found }' /proc/mounts") })
    }

    pub fn opt(&self) -> &'static str {
        if self.read_only { "/oem/open-cgs2/opt" } else { "/opt" }
    }

    fn root(&self) -> &'static str {
        if self.read_only { "/oem/open-cgs2/root" } else { "/root" }
    }
}

fn mode(path: &str) -> u32 {
    if path.ends_with(".sh") || path.ends_with("/panel") || path.ends_with("/sensord") { 0o755 } else { 0o644 }
}

/// Keeps the original of a stock file the first time it is changed.
fn keep_original(d: &mut Device, path: &str) -> Result<(), String> {
    d.sh(&format!("o={STATE}/orig{path}; mkdir -p ${{o%/*}}; [ -e $o ] || cp -p {path} $o")).map(drop)
}

fn push_files(d: &mut Device, layout: &Layout, bundle: &Bundle) -> Result<(), String> {
    let files: Vec<&(String, Vec<u8>)> = bundle.0.iter().filter(|(p, _)| p.starts_with("open-cgs2/")).collect();
    let ui = format!("{}/open-cgs2/ui", layout.opt());
    d.sh(&format!("rm -rf {ui}"))?;
    for (i, (path, data)) in files.iter().enumerate() {
        print!("\r  {}/{} {path:<60}", i + 1, files.len());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        d.push(data, &format!("{}/{path}", layout.opt()), mode(path))?;
    }
    println!();
    Ok(())
}

pub fn install(d: &mut Device, bundle: &Bundle, o: &Options) -> Result<(), String> {
    let layout = Layout::detect(d)?;
    println!("Installing open-cgs2 {VERSION} on {} ({} firmware)", d.host, if layout.read_only { "read-only" } else { "writable" });
    d.sh(&format!("mkdir -p {STATE}"))?;
    push_files(d, &layout, bundle)?;
    match &o.config {
        Some(c) => d.push(serde_json::to_string_pretty(c).unwrap().as_bytes(), &format!("{STATE}/config.json"), 0o600)?,
        None if !d.test(&format!("[ -e {STATE}/config.json ]")) => d.push(b"{}\n", &format!("{STATE}/config.json"), 0o600)?,
        None => {}
    }
    if layout.read_only {
        if !d.test("[ -e /oem/bin/start-stock.sh ]") {
            d.sh("cp -p /oem/bin/start.sh /oem/bin/start-stock.sh")?;
        }
        d.push(bundle.get("oem-start.sh").ok_or("the bundle has no oem-start.sh")?, "/oem/bin/start.sh", 0o755)?;
        if o.lan_only {
            println!("  LAN-only mode needs a writable /etc, skipped on this firmware");
        }
        if !o.open_adb {
            println!("  Key-only ADB needs a writable /etc; ADB stays open to anyone on this firmware");
        }
    } else {
        keep_original(d, "/etc/init.d/S50launcher")?;
        d.sh(r#"grep -q open-cgs2 /etc/init.d/S50launcher || sed -i 's#^\([[:space:]]*\)source /qingping/bin/start.sh#\1if [ -f /opt/open-cgs2/boot.sh ]; then . /opt/open-cgs2/boot.sh; else source /qingping/bin/start.sh; fi#' /etc/init.d/S50launcher"#)?;
        if !d.test("grep -q /opt/open-cgs2/boot.sh /etc/init.d/S50launcher") {
            return Err("could not add the boot hook to /etc/init.d/S50launcher".into());
        }
        if d.test("[ -e /etc/resolv.conf.tail ]") {
            keep_original(d, "/etc/resolv.conf.tail")?;
            d.sh("rm /etc/resolv.conf.tail")?;
        }
        if o.lan_only {
            keep_original(d, "/etc/dhcpcd.conf")?;
            d.sh(&format!("grep -q '^nogateway' /etc/dhcpcd.conf || echo nogateway >> /etc/dhcpcd.conf; touch {STATE}/lan-only"))?;
        } else if d.test(&format!("[ -e {STATE}/lan-only ]")) {
            d.sh(&format!("sed -i '/^nogateway/d' /etc/dhcpcd.conf; rm {STATE}/lan-only"))?;
        }
        if !o.open_adb {
            let public = key::public()?;
            keep_original(d, "/etc/init.d/S50usbdevice")?;
            d.sh(&format!("touch /adb_keys; grep -qF '{0}' /adb_keys || echo '{0}' >> /adb_keys", public.split(' ').next().unwrap()))?;
            d.sh(r#"grep -q adbd-auth /etc/init.d/S50usbdevice || sed -i 's#^\([[:space:]]*\)start-stop-daemon --start --quiet --background --exec /usr/bin/adbd#\1ADBD_RSA_AUTH_ENABLE=1 LD_PRELOAD=/opt/open-cgs2/adbd-auth.so start-stop-daemon --start --quiet --background --exec /usr/bin/adbd#' /etc/init.d/S50usbdevice"#)?;
            println!("  ADB now only accepts this PC's key ({})", key::path().display());
        }
    }
    if let Some(k) = &o.ssh_key {
        let root = layout.root();
        d.sh(&format!("mkdir -p {root}/.ssh && chmod 700 {root}/.ssh && touch {root}/.ssh/authorized_keys && (grep -qF '{0}' {root}/.ssh/authorized_keys || echo '{0}' >> {root}/.ssh/authorized_keys) && chmod 600 {root}/.ssh/authorized_keys", k.trim()))?;
    }
    println!("Installed. It starts on the next boot.");
    Ok(())
}

pub fn update(d: &mut Device, bundle: &Bundle) -> Result<(), String> {
    let layout = Layout::detect(d)?;
    if !d.test(&format!("[ -e {}/open-cgs2/boot.sh ]", layout.opt())) {
        return Err(format!("open-cgs2 is not installed on {}", d.host));
    }
    println!("Updating {} to open-cgs2 {VERSION}", d.host);
    push_files(d, &layout, bundle)?;
    restart(d)
}

pub fn restart(d: &mut Device) -> Result<(), String> {
    if d.test("[ \"$(cat /run/open-cgs2-mode 2>/dev/null)\" = open-cgs2 ]") {
        d.sh("sh /opt/open-cgs2/start.sh supervisor; killall panel sensord qmlscene 2>/dev/null; true")?;
        println!("Restarted.");
    }
    Ok(())
}

pub fn uninstall(d: &mut Device, purge: bool) -> Result<(), String> {
    let layout = Layout::detect(d)?;
    println!("Removing open-cgs2 from {}", d.host);
    if layout.read_only {
        d.sh("[ -e /oem/bin/start-stock.sh ] && mv /oem/bin/start-stock.sh /oem/bin/start.sh; rm -rf /oem/open-cgs2")?;
    } else {
        d.sh(&format!("cd {STATE}/orig 2>/dev/null && find . -type f | while read f; do cp -p \"$f\" \"/${{f#./}}\"; done; true"))?;
        d.sh("rm -rf /opt/open-cgs2")?;
    }
    if purge {
        d.sh(&format!("rm -rf {STATE}"))?;
    }
    println!("Done; the stock firmware starts on the next boot.");
    Ok(())
}

pub fn status(d: &mut Device) -> Result<(), String> {
    let layout = Layout::detect(d)?;
    let installed = d.sh(&format!("cat {}/open-cgs2/VERSION 2>/dev/null || echo none", layout.opt())).unwrap_or_default();
    let mode = d.sh("cat /run/open-cgs2-mode 2>/dev/null || echo stock").unwrap_or_default();
    let firmware = d.sh("sed -n 's/^CLEARGRASS_VERSION=//p' /qingping/etc/os-release").unwrap_or_default();
    println!(
        "{}: firmware {} ({}), running {}, open-cgs2 {}",
        d.host,
        firmware.trim(),
        if layout.read_only { "read-only root" } else { "writable root" },
        mode.trim(),
        installed.trim()
    );
    Ok(())
}

pub fn logs(d: &mut Device, name: &str) -> Result<(), String> {
    if !["panel", "sensord", "screen", "supervisor", "boot"].contains(&name) {
        return Err(format!("no log named {name}"));
    }
    print!("{}", d.sh(&format!("tail -n 200 {STATE}/{name}.log"))?);
    Ok(())
}

pub fn push_config(d: &mut Device, config: &Value) -> Result<(), String> {
    d.push(serde_json::to_string_pretty(config).unwrap().as_bytes(), &format!("{STATE}/config.json"), 0o600)?;
    restart(d)
}

pub fn device_config(d: &mut Device) -> Value {
    d.sh(&format!("cat {STATE}/config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
}

/// Reads a config.json and gives it the household key unless it has one or intercom is off.
fn read_config(path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut c: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let intercom = &mut c["components"]["intercom"];
    match intercom {
        Value::Bool(false) => {}
        Value::Object(o) if o.contains_key("key") || o.get("enabled") == Some(&json!(false)) => {}
        Value::Object(o) => drop(o.insert("key".into(), json!(wizard::household_key()?))),
        _ => *intercom = json!({"enabled": intercom.as_bool().map_or(json!("auto"), |b| json!(b)), "key": wizard::household_key()?}),
    }
    Ok(c)
}

fn run(args: &[String]) -> Result<(), String> {
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let host = || args.get(1).filter(|h| !h.starts_with("--")).cloned().ok_or_else(|| HELP.to_string());
    match args.first().map(String::as_str) {
        None => wizard::run(),
        Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            Ok(())
        }
        Some("--version") => {
            println!("cgs2 {VERSION}");
            Ok(())
        }
        Some("status") => status(&mut Device::connect(&host()?)?),
        Some("install") => {
            let bundle = Bundle::load(value("--bundle").as_deref())?;
            let ssh_key = value("--ssh-key").map(|p| std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))).transpose()?;
            let config = value("--config").map(|p| read_config(&p)).transpose()?;
            let mut d = Device::connect(&host()?)?;
            install(&mut d, &bundle, &Options { config, lan_only: flag("--lan-only"), open_adb: flag("--open-adb"), ssh_key })
        }
        Some("update") => update(&mut Device::connect(&host()?)?, &Bundle::load(value("--bundle").as_deref())?),
        Some("config") => {
            let mut d = Device::connect(&host()?)?;
            let config = match args.get(2) {
                Some(path) => read_config(path)?,
                None => wizard::config(&device_config(&mut d), &Bundle::load(None).map(|b| b.components()).unwrap_or_default())?,
            };
            push_config(&mut d, &config)
        }
        Some("logs") => logs(&mut Device::connect(&host()?)?, args.get(2).map_or("panel", String::as_str)),
        Some("uninstall") => uninstall(&mut Device::connect(&host()?)?, flag("--purge")),
        Some(other) => Err(format!("unknown command {other}\n\n{HELP}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let interactive = args.is_empty();
    let result = run(&args);
    if let Err(e) = &result {
        eprintln!("error: {e}");
    }
    if interactive {
        // A double-clicked console window would close before the last message could be read.
        let _ = dialoguer::Input::<String>::new().with_prompt("Press Enter to close").allow_empty(true).interact_text();
    }
    std::process::exit(result.is_err() as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_bundle_directory() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../device");
        let b = Bundle::load(Some(dir.to_str().unwrap())).unwrap();
        assert!(b.get("oem-start.sh").is_some() && b.get("open-cgs2/boot.sh").is_some());
        let names: Vec<String> = b.components().into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"weather".to_string()) && names.contains(&"path".to_string()));
        assert_eq!(mode("open-cgs2/panel"), 0o755);
        assert_eq!(mode("open-cgs2/ui/qml/main.qml"), 0o644);
    }
}
