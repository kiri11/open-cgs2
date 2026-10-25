# Installing

## What you need

- A Qingping Air Monitor 2 (CGS2) on your Wi-Fi.
- ADB turned on: **Settings → About**, tap **Device Name** 7 times, then **Developer options → ADB**.
  ADB then answers as root on USB-C and on port 5555 of the device's address, with no password.
- A computer on the same network. The installer has its own ADB client, so Google's adb is not
  needed.

## The installer

Download `cgs2` for your system from the releases and run it with no arguments for the
step-by-step setup. It scans the network for devices with ADB, asks for the settings and installs.
The same program has commands for scripts:

```bash
cgs2 status 192.0.2.10
```

```bash
cgs2 install 192.0.2.10 --config config.json
```

| Command | What it does |
|---|---|
| `cgs2 status HOST` | Firmware, layout, the mode it runs and the installed version. |
| `cgs2 install HOST` | Installs, or reinstalls keeping the device's settings. `--config FILE` pushes a config.json, `--lan-only` keeps the device off the internet, `--open-adb` leaves ADB open, `--ssh-key FILE` allows a public key over SSH, `--bundle DIR` installs device files from a folder. |
| `cgs2 update HOST` | New device files; settings and history stay. |
| `cgs2 config HOST [FILE]` | Asks for the settings again, or pushes FILE as config.json, and restarts the screen. |
| `cgs2 logs HOST [NAME]` | The last lines of `panel`, `sensord`, `screen`, `supervisor` or `boot`. |
| `cgs2 screenshot HOST FILE [VIEW]` | Saves the screen as a PNG, or a view: a page (`weather`, ...), `music`, `air`, `chart`, `clock` or `settings`. |
| `cgs2 fonts HOST [DIR]` | Copies the device's fonts for the [emulator](components.md#the-emulator) (`emulator/fonts`). |
| `cgs2 uninstall HOST [--purge]` | Restores the stock firmware's files; `--purge` also deletes the settings and history. |

The installer keeps its ADB key in `~/.android/adbkey`, the same file Google's adb uses, and makes
one if there is none. Intercom units find each other with a household key that the installer
keeps next to its settings (`%APPDATA%\open-cgs2\household.key` on Windows,
`~/.config/open-cgs2/household.key` elsewhere), so install every unit from the same computer, or copy
that file.

## What it changes on the device

| Path | |
|---|---|
| `/opt/open-cgs2/` | The programs, scripts and screen. On firmware with a read-only root this is `/oem/open-cgs2/opt/open-cgs2/`, mounted over `/opt` at boot. |
| `/userdata/open-cgs2/` | `config.json`, logs, sensor history and saved state. |
| `/etc/init.d/S50launcher` | One line: the stock start script is sourced through `boot.sh` while `/opt/open-cgs2/boot.sh` exists. On read-only firmware, `/oem/bin/start.sh` instead, with the stock script moved to `/oem/bin/start-stock.sh`. |
| `/etc/init.d/S50usbdevice`, `/adb_keys` | Key-only ADB: adbd checks keys against `/adb_keys`, with `adbd-auth.so` preloaded (the firmware's adbd crashes verifying a key without it). Skipped with `--open-adb` and on read-only firmware. |
| `/etc/resolv.conf.tail` | Removed: it adds about fifteen public DNS servers after your router's. |
| `/etc/dhcpcd.conf` | `nogateway` in LAN-only mode. |

Every stock file is copied to `/userdata/open-cgs2/orig/` before its first change, and `cgs2
uninstall` puts them back.

The stock app, its watchdog, the Mi Home client and the noise sampler do not run in open-cgs2 mode.
`dropbear` runs with password logins off; add a key with `--ssh-key` (the stock `dropbear` allows
root logins with a published password).

## Firmware layouts

Two layouts are known, and the installer finds out which one a device has:

- **Writable root** (`/etc` can be changed): open-cgs2 lives in `/opt/open-cgs2` and hooks into
  `/etc/init.d/S50launcher`.
- **Read-only root** (a squashfs, with `/oem` and `/userdata` writable): the read-only launcher
  sources `/qingping/bin/start.sh`, which is on `/oem`, so that is where the hook goes. ADB stays
  open on these, LAN-only mode is not available, and two things the firmware lacks can be added:
  - `/oem/open-cgs2/opt/open-cgs2/ntpd`: a static busybox (for example Alpine's `busybox-static`,
    renamed to `ntpd` so it runs as its ntpd applet). The supervisor runs it against `pool.ntp.org`,
    or the server in `/userdata/open-cgs2/ntp-server`.
  - `/oem/open-cgs2/opt/open-cgs2/qt/plugins/imageformats/libqjpeg.so`, copied from a device with a
    writable root, so album art decodes.

## Boot, recovery and going back

Each boot counts itself in `/userdata/open-cgs2/bootcount`; the supervisor resets the count once
`sensord` has run for a minute. The stock firmware boots instead when:

- `/userdata/open-cgs2/stock` exists (the kill switch),
- Settings → **Restart into stock firmware** asked for one stock boot,
- or open-cgs2 failed to come up three boots in a row. Without ADB, power-cycle the device within a
  minute of boot three times in a row.

A boot that is not healthy after 5 minutes reboots, and `sensord` feeds the hardware watchdog, so
a device that hangs restarts by itself. Back to open-cgs2 after the kill switch:

```bash
adb shell 'rm -f /userdata/open-cgs2/stock /userdata/open-cgs2/bootcount; reboot'
```

## LAN only

With `--lan-only` the device gets no default route and no IPv6, so it only talks to your network.
The weather and PATH components then need a proxy on your network: point their `source` (PATH) at
it, or use Home Assistant's weather entity. `examples/fetch-proxy.py` is such a proxy.

## Building from source

The device programs are Rust, built as static aarch64 binaries on Linux, macOS or WSL:

```bash
rustup target add aarch64-unknown-linux-musl
```

```bash
scripts/bundle.sh
```

That leaves the device files in `dist/device` and `dist/bundle.tar`. Install them with an
installer built from source:

```bash
cargo run -p cgs2 -- install 192.0.2.10 --bundle dist/device
```

or build them into the installer with `CGS2_BUNDLE=dist/bundle.tar cargo build --release -p cgs2`.
