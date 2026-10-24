# The device and how open-cgs2 drives it

The Qingping Air Monitor 2 (CGS2) runs Buildroot 2018.02 on a Rockchip PX30 (four Cortex-A35
cores, Mali-G31), kernel 4.19, 465 MB of RAM and Qt 5.14.2, with a 720×720 touchscreen, a battery,
Wi-Fi, a microphone and a speaker. The stock app, `QingSnow2App`, measures, reports to the cloud
and draws the screen. In open-cgs2 mode three programs replace it:

- `sensord` measures, corrects, logs and reports the readings.
- `panel` is the screen's backend: Home Assistant, Assist, the intercom, the backlight, clock and
  alarms, and the HTTP API the screen reads.
- `qmlscene -platform eglfs main.qml` draws the screen with the firmware's own Qt.

`supervisor.sh` keeps them running, along with Wi-Fi and DHCP.

## Boot

`/etc/init.d/S50launcher` sources the stock start script. open-cgs2's hook (see
[Installing](install.md#what-it-changes-on-the-device)) sources `boot.sh` instead, which picks the
mode for this boot and either runs the stock script or `start.sh boot`. `start.sh` does the stock
hardware bring-up (CO₂ sensor reset, Wi-Fi power and module, audio paths, `wpa_supplicant`) without
the stock app, its watchdog, the Mi Home client (`miio_client`, which drops Wi-Fi when its cloud is
unreachable) and the noise sampler, then starts `dropbear` with password logins off and the
supervisor. `/run/open-cgs2-mode` holds the mode.

The supervisor restarts `sensord` within 5 seconds of exiting, the screen within 10, and
`wpa_supplicant` and `dhcpcd` when they are gone. When the router has not answered a ping for 2
minutes it restarts Wi-Fi, and again every 2 minutes until the router answers; it never reboots for
this. Logs are in `/userdata/open-cgs2/`: `boot.log`, `supervisor.log`, `sensord.log`, `panel.log`
and `screen.log`.

Firmware with a read-only root lacks a JPEG plugin for Qt and `ntpd`; see
[Installing](install.md#firmware-layouts).

## Sensors

The sensors have kernel drivers under `/sys/bus/i2c/devices/`: SHT4x temperature and humidity
(`1-0044`), OPT3004 light (`1-0045`), SGP40 VOC (`1-0059`), SCD40 CO₂ (`1-0062`), the particulate
sensor (`3-0012`) and a pressure sensor (`1-0077`, reads zeros). The drivers busy-wait while a
sensor measures (about 0.1 s of CPU per CO₂ read) and cut values to tenths, so `sensord` talks to
the four sensors on bus 1 itself through `/dev/i2c-1`, leaving the drivers bound and unused for the
stock firmware. Side by side, the two agreed, and the CPU time went from about 5.4% of a core to
under 0.2%. The particulate sensor stays on its cheap driver, read in a thread of its own so that it
never stalls the others (the stock app serialised every read behind one lock).

`sensord` reads temperature and humidity every 2 seconds, VOC and light every second, CO₂ whenever
the SCD40 has a sample (every 5 seconds) and the particulate sensor every second. It restarts the
SCD40 when it has had no sample for 15 seconds and reports CO₂ between 400 and 9999 ppm. The light
reading is ten times the OPT3004's datasheet lux, as the stock driver reported it (presumably for
its window); the backlight curve is on that scale.

### Temperature and humidity

Temperature is corrected for the case's own heat with the stock app's models, read from the
device (`/qingping/etc/models.ini`): time since power-on (up to −1.5° after 55 minutes), charging,
and the backlight averaged over about 25 minutes. Humidity is then moved to the corrected
temperature at the same dew point. On two units, next to an Aranet4 for hours and against a
thermostat, the stock humidity read 7 to 9 points high: the SHT4x reads about 10% too much water
vapour. `calibration.humidity_scale` (0.9) scales the raw reading before the move. The correction's
state is saved every 10 minutes and on exit and restored if it is less than an hour old; without
one, `sensord` starts from the stock app's own saved state.

### VOC

The VOC index is Sensirion's VOC algorithm with the stock app's tuning, stepped once a second and
remapped as the app did, so steady air reports about 52. The stock app relearned its baseline at
every start and then reported about average air, whatever the air did, for a day or more. `sensord`
saves what it learned every 10 minutes and on exit and carries on from it. Over two weeks of
restarts on two units, the saved baseline followed CO₂ (a stand-in for people in the room) better in
the first day after a restart: a correlation of 0.55 and 0.48, against 0.28 and 0.40 for an engine
learning from scratch.

After power-on the SGP40 reads low while it heats up, which a saved baseline takes for bad air (for
about 3 minutes after a reboot, and up to half an hour after a long time off). So within 30 minutes
of power-on no VOC index is reported until the raw signal levels off: the last 5 minutes average
less than 20 counts above the 5 before.

### Particulate matter and CO₂ calibration

The particulate sensor is off at night (`particulate.night`, or Settings). The screen switches it
through `/run/open-cgs2/pm/cmd` and shows `/run/open-cgs2/pm/status`. Readings are held back for 30
seconds after power-on, and 5 failed reads in a row report it as not responding.

The SCD40 keeps its automatic self-calibration on, which takes the lowest level it sees over about
a week as 400 ppm. Settings can also force a recalibration to outdoor air (420 ppm) after the
device has stood outdoors for a few minutes; the sensor must have measured for 3 minutes first.

### Watchdog, battery and clock

`sensord` holds `/dev/watchdog` open and writes to it every second while its sensor thread keeps
going; if that thread stalls for 60 seconds the watchdog reboots the device. It powers off, saving
its state first, once the battery is below 20% (shown as 1%, the stock threshold) while not
charging, and copies the system clock, which NTP sets, to the RTC every 6 hours.

### Files

| | |
|---|---|
| `/run/open-cgs2/sensors` | Current readings, as the screen shows them and Home Assistant gets them. |
| `/userdata/open-cgs2/sensord.csv` | A row a minute with the readings, raw values, correction terms and battery, moved to `.1` once its first row is 14 days old, so 2 to 4 weeks are kept. No rows while the clock is unset. |
| `/userdata/open-cgs2/hourly/<year>.csv` | Mean, min and max of each reading per hour, kept for good; paused below 20 MB free. |
| `/userdata/open-cgs2/*.state` | The VOC baseline, the temperature correction, the particulate sensor's switches, the last CO₂ calibration. |

## Screen

The panel serves the screen on `127.0.0.1:8099`: `GET /state?v=<version>` long-polls a JSON
snapshot, and the rest are what the views ask for (`/forecast`, `/library`, `/history/<key>`,
`/settings`, `/fetch?url=`) or send (`/press/<group>`, `/brightness/<group>/<1..255>`, `/assist`,
`/music/<action>`, `/climate/<action>/<value>`, `/ha/call/<domain>/<service>`, `/calibration`,
`/clock/…`, `/intercom/talk`, …).

The panel owns the backlight (`/sys/class/backlight/backlight/brightness`). The level follows the
light with the stock curve, on the mean of the last 10 readings. On battery the screen turns off
after a few minutes without a touch (Settings); plugging or unplugging the charger lights it. A
short press of the power key switches the screen; holding it opens the menu to power off, restart
or open Settings. A screen still lit at midnight turns off until 8 am, and while the screen is dark a
tap only wakes it.

Assist records from the microphone with `arecord`, lets Home Assistant's voice activity detection
end the request and streams the reply to the speaker with `gst-launch-1.0`. Timers and alarms run in
the panel, so they ring with the screen dark; their sounds are the stock ringtones on the device.

## Intercom

Holding Talk streams the microphone to every other unit: mono 16-bit PCM at 16 kHz in 20 ms
chunks over TCP port 8098, with automatic gain. Units sharing a household key announce themselves
every 10 seconds by UDP broadcast on the same port, signed with HMAC-SHA256, and each stream starts
with a signed header, so only units with the key are found and heard. A unit is half-duplex: its
speaker stays quiet while it talks, and streams that arrive meanwhile play after release.
