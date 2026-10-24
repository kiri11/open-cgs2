# Components

The screen is a home page with the device's readings and clock, plus whatever the components add.
A component is a folder in `device/open-cgs2/ui/components/` (on the device,
`/opt/open-cgs2/ui/components/`):

```
components/
  quote/
    component.json
    Page.qml
```

`component.json` says where its QML goes and whether it is on by default:

```json
{
  "title": "Quote of the day",
  "default": true,
  "position": "right",
  "page": "Page.qml"
}
```

| Key | |
|---|---|
| `title` | The name the installer shows. |
| `default` | `true`, `false` or `"auto"`. `auto` turns on when Home Assistant is set up; the built-in components check for more (see [Configuration](configuration.md#components)). config.json's `components.<name>` overrides it. |
| `position` | Where the `page` goes from home: `left`, `right`, `up` or `down`. config.json's `pages` overrides it. |
| `page` | A full-screen page (720×720). |
| `status` | A small item in the row at the top left of the home page. |
| `button` | A round button left of the clock. |
| `side` | An item in the column on the right of the home page (the Assist button, the intercom's Talk). |
| `tiles` | The grid at the bottom of the home page (the lights). |
| `overlay` | A full-screen item above everything, for things that pop up (the doorbell) or open from a button (Spotify). |
| `order` | Order among the status, side and button items (smaller first, 50 by default). |
| `layer` | Stacking among overlays (bigger on top); below 50 an alarm that goes off covers it. |

Every item the screen loads gets two properties, so declare them: `app`, the screen, and `config`,
the component's object from config.json (`{}` when there is none). Pages also get `active` (shown
and the screen on); status items get `room` (the width left before the buttons); side items get
`compact` (they share the column).

## What `app` offers

| | |
|---|---|
| `app.st` | The panel's state: `sensors`, `groups`, `music`, `climate`, `calendar`, `clock`, `pm`, `intercom`, `entities` and more, updated as it changes. |
| `app.entity(id)` | `{state, attributes}` of a Home Assistant entity listed in this component's `entities` setting. |
| `app.callService(domain, service, target, data)` | Calls a Home Assistant service. |
| `app.fetch(url, done)` | Fetches any http(s) URL through the panel, which has the CA certificates, and calls `done` with the parsed JSON, or `null`. |
| `app.get(path, done)`, `app.post(path, body)` | The panel's own HTTP API on 127.0.0.1:8099. |
| `app.sensor(key)` | A reading as a number or `null`: `co2`, `temp`, `rh`, `tvoc`, `pm25`, `pm10`, `battery`, `outside`, `outside_rh`. |
| `app.nowMs`, `app.untilText(ms)` | The time, ticking every second, and "in 5 min"-style text. |
| `app.fontDin`, `app.fontFamily`, `app.fontMedium` | The device's fonts. |
| `app.online`, `app.screenOn`, `app.fahrenheit` | Home Assistant connected, backlight on, temperature unit. |
| `app.modal` | The open full-screen component (`""` for none): set it from a button, clear it to close. |
| `app.openPage(name, arg)`, `app.page(name)` | Swipes to a component's page and calls its `open(arg)`; the page's item. |
| `app.openClock()`, `app.pageTouched()`, `app.goHome()` | Open the clock, keep a page from timing out, go home. |

`Fmt.time(date)` and `Fmt.hm("19:30")` format times for the 12- or 24-hour clock, and the shared
items in `ui/qml` (`Icon`, `WeatherIcon`, `Toggle`, `SettingsButton`, …) are there with
`import "../../qml"`.

## A component without a backend

Most components need no Rust: QML can draw, animate, keep timers and, through `app`, read Home
Assistant and the web. This one shows a quote of the day from a public API:

```qml
import QtQuick 2.14
import "../../qml"

Item {
    property var app
    property var config
    property bool active
    property var quote: null

    onActiveChanged: if (active) app.fetch(config.source || "https://zenquotes.io/api/today", function(v) { quote = v ? v[0] : null; })

    Text {
        anchors.centerIn: parent
        width: 600
        wrapMode: Text.WordWrap
        horizontalAlignment: Text.AlignHCenter
        text: quote ? "“" + quote.q + "”\n\n" + quote.a : "…"
        font.family: app.fontFamily
        font.pixelSize: 40
        color: "#e6ebf1"
    }
}
```

And one that follows Home Assistant entities, a tile row for two fans, with `entities` in
config.json so the panel subscribes to them:

```json
"components": {"fans": {"entities": ["fan.bedroom", "fan.office"]}}
```

```qml
Row {
    property var app
    property var config
    spacing: 16
    Repeater {
        model: config.entities
        Rectangle {
            width: 208; height: 160; radius: 24
            color: app.entity(modelData).state === "on" ? "#3fa9ff" : "#161b22"
            MouseArea { anchors.fill: parent; onClicked: app.callService("fan", "toggle", { entity_id: modelData }) }
        }
    }
}
```

The PATH component (`components/path`) is a complete example of this kind.

## A component with a backend

Anything that needs the microphone or speaker, the backlight, a protocol QML cannot speak (the
intercom streams audio between units) or work that goes on while its page is closed (calendar
alerts, the doorbell) needs a part in the panel, in Rust:

1. Add a module to `panel/src` with a `run(shared)` thread, or a piece of `ha.rs`'s session if it
   talks to Home Assistant, and start it from `main.rs` when its component is enabled. Its
   settings come from `Config` (`panel/src/config.rs`), which reads `components.<name>`.
2. Keep its state in `State`, and put what the screen needs in `snapshot()` under a key of its own;
   the screen gets every change through the long-polled `/state`.
3. Add HTTP routes in `main.rs`'s `handle()` for what the screen sends.
4. If it can turn on by itself, add a rule to `auto()` in `config.rs`, and to `detect.rs` if it looks
   for something in Home Assistant.
5. Test the logic with `cargo test -p panel` and the QML with the emulator.

## The emulator

`emulator/emulator.py` runs the screen on a computer against a stand-in for the panel with made-up
state, so components can be built and tried without a device. It needs Python 3 and PyQt5
(`pip install PyQt5`):

```bash
python emulator/emulator.py
```

```bash
python emulator/emulator.py --view weather --shot weather.png
```

`--config` takes a config.json (the default is `emulator/demo.json`) for the components, pages
and units. For the device's look, copy its fonts (`/usr/share/fonts/DIN_Condensed_QP.ttf`,
`Roboto-Regular.ttf` and `Roboto-Medium.ttf`) into `emulator/fonts`. Components that only use
`app` work unchanged; one with a backend needs its state added to the emulator's `Panel`.

On a device, `cgs2 update HOST --bundle dist/device` after `scripts/bundle.sh` installs your
change, or copy just the component's folder and restart the screen with `killall qmlscene`.
