"""Runs the screen on a PC against a stand-in for the panel with made-up data.

    python emulator/emulator.py                    # a 720x720 window
    python emulator/emulator.py --view weather --shot weather.png

Needs Python 3 and PyQt5. The demo config (emulator/demo.json) picks the components, as
config.json does on a device; components without a backend of their own work unchanged.
"""
import argparse
import json
import math
import os
import sys
import threading
import time
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
UI = HERE.parent / "device/open-cgs2/ui"
PORT = 8099


def components(config):
    """The enabled components, as the panel resolves them, with `auto` taken as on."""
    out = []
    for d in sorted((UI / "components").iterdir()):
        manifest = json.loads((d / "component.json").read_text(encoding="utf-8"))
        opts = config.get("components", {}).get(d.name, {})
        opts = {"enabled": opts} if isinstance(opts, bool) else opts
        if opts.get("enabled", manifest.get("default")) is False:
            continue
        c = {"name": d.name, "position": manifest.get("position"), "order": manifest.get("order", 50),
             "layer": manifest.get("layer", 0), "config": opts}
        for slot in ("page", "status", "button", "side", "tiles", "overlay"):
            if slot in manifest:
                c[slot] = manifest[slot]
        out.append(c)
    return out


def forecast(now):
    hour = now - now % 3600
    hourly = []
    for i in range(48):
        t = hour + i * 3600
        rain = max(0.0, math.sin((i - 5) / 3)) * 1.5 if 4 < i < 14 else 0.0
        hourly.append({"t": t, "condition": "rainy" if rain > 0.2 else "partlycloudy" if i % 7 else "sunny",
                       "temp": round(14 + 5 * math.sin((i - 9) / 24 * 2 * math.pi), 1), "rain": round(rain, 1),
                       "chance": 70 if rain > 0.2 else 10})
    day = now - now % 86400
    daily = [{"t": day + i * 86400, "condition": ["sunny", "rainy", "cloudy", "partlycloudy"][i % 4], "temp": 17 + i % 3,
              "low": 9 + i % 2, "rain": [0, 4.2, 0.5, 0][i % 4], "chance": [5, 80, 30, 10][i % 4]} for i in range(8)]
    return {"hourly": hourly, "daily": daily}


def history(key, now):
    base = {"co2": (620, 380), "temp": (21.5, 1.5), "rh": (45, 6), "tvoc": (55, 25), "pm25": (4, 3), "pm10": (6, 4)}[key]
    count = 180
    def series(name, shift, here=False):
        return {"name": name, "here": here, "outside": False,
                "values": [round(base[0] + base[1] * math.sin((i + shift) / 25), 1) for i in range(count)]}
    return {"key": key, "start": now - count * 60, "step": 60, "ticks": [{"i": i, "label": f"{(i // 60) % 24:02}:00"} for i in (40, 100, 160)],
            "series": [series("Living Room", 0, True), series("Bedroom", 30)]}


def ridepath():
    msgs = lambda heads: [{"headSign": h, "lineColor": c, "secondsToArrival": str(s)} for h, c, s in heads]
    return {"results": [{"consideredStation": "GRV", "destinations": [
        {"label": "ToNY", "messages": msgs([("World Trade Center", "D93A30", 240), ("33rd Street", "FF9900", 420), ("World Trade Center", "D93A30", 900)])},
        {"label": "ToNJ", "messages": msgs([("Newark", "D93A30", 300), ("Journal Square via Hoboken", "4D92FB,FF9900", 660)])}]}]}


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        pass  # the screen dropping a long poll when it closes


class Panel:
    def __init__(self, config):
        now = int(time.time())
        self.cond = threading.Condition()
        self.calibration = {"humidity_scale": 0.9, "temperature_offset": 0.0, "humidity_offset": 0.0}
        self.state = {
            "v": 1, "name": config.get("device", {}).get("name", "CGS2"), "ha": True, "screen": True,
            "units": config.get("units", {"temperature": "C", "clock": "24h"}), "components": components(config),
            "pages": config.get("pages"), "location": config.get("location"), "entities": {}, "power_menu": False,
            "sensors": {"co2": 712, "temp": 21.8, "rh": 44.0, "tvoc": 48, "battery": 100, "outside": 15.2, "outside_rh": 71,
                        "sky": "partlycloudy", "rain_soon": "Rain starting in 45 min"},
            "groups": [
                {"id": "living_room", "name": "Living Room", "on": True, "available": True, "off_only": False, "rgb": [255, 196, 120], "brightness": 180, "dimmable": True},
                {"id": "kitchen", "name": "Kitchen", "on": False, "available": True, "off_only": False, "rgb": None, "brightness": 0, "dimmable": True},
                {"id": "bedroom", "name": "Bedroom", "on": False, "available": True, "off_only": False, "rgb": None, "brightness": 0, "dimmable": True},
                {"id": "office", "name": "Office", "on": True, "available": True, "off_only": False, "rgb": [200, 220, 255], "brightness": 255, "dimmable": True},
                {"id": "hall", "name": "Hall", "on": False, "available": True, "off_only": False, "rgb": None, "brightness": 0, "dimmable": False},
                {"id": "all_off", "name": "All off", "on": True, "available": True, "off_only": True, "rgb": None, "brightness": 0, "dimmable": False}],
            "assist": {"phase": "idle", "heard": "", "reply": "", "error": ""},
            "music": {"active": "living_room", "state": "playing", "selected": "living_room", "title": "Clair de Lune", "artist": "Claude Debussy",
                      "art": None, "volume": 35, "shuffle": False, "track": None, "library_v": 1, "current": None,
                      "speakers": [{"id": "living_room", "name": "Living Room", "state": "playing"}, {"id": "kitchen", "name": "Kitchen", "state": "idle"},
                                   {"id": "bedroom", "name": "Bedroom", "state": "unavailable"}]},
            "climate": {"mode": "heat", "hvac_modes": ["off", "heat", "cool", "heat_cool"], "hvac_action": "heating", "min_temp": 9, "max_temp": 32,
                        "current_temperature": 20.5, "current_humidity": 46, "temperature": 21.5, "fan_mode": "off", "updated": now},
            "calendar": [{"summary": "Dentist", "all_day": False, "start": now + 2 * 3600, "end": now + 3 * 3600}],
            "pm": {"state": "on", "enabled": True, "night": True, "from": "22:00", "until": "08:30", "pm1": 2, "pm25": 4, "pm10": 6},
            "clock": {"alarms": [{"id": 1, "time": "07:30", "days": [1, 2, 3, 4, 5], "tone": "", "on": True}], "timers": [], "ringing": None, "snoozed": None},
            "intercom": {"peers": ["Bedroom"], "talking": False, "links": [], "incoming": []},
            "doorbell": None, "call": None,
        }

    def change(self, f):
        with self.cond:
            f(self.state)
            self.state["v"] += 1
            self.cond.notify_all()

    def assist(self):
        steps = [("listening", "", ""), ("thinking", "What's the weather like?", ""),
                 ("speaking", "What's the weather like?", "It's 15 degrees and partly cloudy, with rain from this afternoon."), ("done", None, None)]
        for phase, heard, reply in steps:
            def set_phase(s, phase=phase, heard=heard, reply=reply):
                s["assist"]["phase"] = phase
                if heard is not None:
                    s["assist"].update(heard=heard, reply=reply)
            self.change(set_phase)
            time.sleep(2)
        self.change(lambda s: s["assist"].update(phase="idle"))


def handler(panel):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def reply(self, body, status=200, kind="application/json"):
            data = body if isinstance(body, bytes) else json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            url = urllib.parse.urlparse(self.path)
            q = urllib.parse.parse_qs(url.query)
            now = int(time.time())
            if url.path == "/state":
                since = int(q.get("v", ["0"])[0])
                with panel.cond:
                    panel.cond.wait_for(lambda: panel.state["v"] != since, timeout=25)
                    body = json.dumps(panel.state).encode()
                return self.reply(body)
            if url.path == "/forecast":
                return self.reply(forecast(now))
            if url.path == "/library":
                return self.reply({"playlists": [{"name": n, "uri": "spotify:playlist:" + n} for n in ("Morning", "Focus", "Dinner", "Running")],
                                   "recent": [], "mixes": [], "recent_supported": False})
            if url.path.startswith("/history/"):
                v = history(url.path.split("/")[-1], now)
                v["range"] = q.get("range", ["3h"])[0]
                return self.reply(v)
            if url.path == "/settings":
                return self.reply({"name": panel.state["name"], "version": "emulator", "adjust": panel.calibration,
                                   "wifi": {"connected": True, "ssid": "Home", "ip": "192.0.2.10", "rssi": -52}, "mac": "00:00:5e:00:53:01",
                                   "battery": 100, "charging": True, "co2": 712, "uptime": 86400, "firmware": "4.5.6", "calibration": {},
                                   "screen": {"auto": True, "level": 500, "idle": 5}})
            if url.path == "/clock/tones":
                return self.reply([{"key": "chime", "name": "Chime"}, {"key": "bell", "name": "Bell"}])
            if url.path == "/wifi/scan":
                return self.reply([{"ssid": "Home", "rssi": -52, "secure": True}])
            if url.path == "/fetch":
                target = q.get("url", [""])[0]
                if "ridepath" in target:
                    return self.reply(ridepath())
                try:
                    with urllib.request.urlopen(target, timeout=15) as r:
                        return self.reply(r.read(), kind=r.headers.get("Content-Type", "application/octet-stream"))
                except Exception as e:
                    return self.reply({"error": str(e)}, 502)
            self.reply({}, 404)

        def do_POST(self):
            path = urllib.parse.urlparse(self.path).path
            length = int(self.headers.get("Content-Length") or 0)
            body = json.loads(self.rfile.read(length) or b"null") if length else None
            if path.startswith("/press/"):
                gid = path.split("/")[-1]
                def press(s):
                    groups = s["groups"]
                    g = next(x for x in groups if x["id"] == gid)
                    if g["off_only"]:
                        for x in groups:
                            x["on"] = False
                    else:
                        g["on"] = not g["on"]
                        g["brightness"] = 200 if g["on"] else 0
                        g["rgb"] = [255, 196, 120] if g["on"] else None
                    groups[-1]["on"] = any(x["on"] for x in groups[:-1])
                panel.change(press)
            elif path == "/assist":
                if panel.state["assist"]["phase"] == "idle":
                    threading.Thread(target=panel.assist, daemon=True).start()
            elif path == "/dismiss":
                panel.change(lambda s: s["assist"].update(phase="idle"))
            elif path.startswith("/music/") and path.split("/")[2] in ("play", "pause"):
                panel.change(lambda s: s["music"].update(state="playing" if s["music"]["state"] != "playing" else "paused"))
            elif path == "/calibration" and isinstance(body, dict):
                panel.calibration.update(body)
            elif path.startswith("/power/"):
                panel.change(lambda s: s.update(power_menu=False))
            self.reply({})

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", default=str(HERE / "demo.json"), help="config.json to take components, units and pages from")
    parser.add_argument("--fonts", default=str(HERE / "fonts"), help="folder with the device's fonts (DIN_Condensed_QP.ttf, Roboto-*.ttf)")
    parser.add_argument("--view", help="what to show first: a component page (weather, ...), music, air, chart, clock, settings, assist or power")
    parser.add_argument("--shot", help="save a screenshot to this PNG after a few seconds and quit")
    parser.add_argument("--panel", action="store_true", help="use a real panel already listening on 127.0.0.1:8099 instead of made-up state")
    args = parser.parse_args()
    if args.shot:
        # Qt's offscreen platform only renders on Linux; on Windows the window opens off screen.
        if sys.platform != "win32":
            os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
        os.environ.setdefault("QT_QUICK_BACKEND", "software")

    from PyQt5.QtCore import QTimer, QUrl
    from PyQt5.QtGui import QFontDatabase, QGuiApplication
    from PyQt5.QtQml import QQmlApplicationEngine
    from PyQt5.QtQuick import QQuickWindow  # so that the root object comes back as one

    panel = Panel(json.loads(Path(args.config).read_text(encoding="utf-8")))
    if not args.panel:
        server = Server(("127.0.0.1", PORT), handler(panel))
        threading.Thread(target=server.serve_forever, daemon=True).start()

    app = QGuiApplication(sys.argv)
    for font in Path(args.fonts).glob("*.ttf"):
        QFontDatabase.addApplicationFont(str(font))
    engine = QQmlApplicationEngine()
    engine.warnings.connect(lambda ws: [print(w.toString(), file=sys.stderr) for w in ws])
    engine.load(QUrl.fromLocalFile(str(UI / "qml/main.qml")))
    if not engine.rootObjects():
        sys.exit("main.qml did not load")
    window = engine.rootObjects()[0]
    if args.shot:
        window.setPosition(-4000, -4000)
    pass
    def show():
        view = args.view
        if view in ("music", "air", "chart", "clock", "settings"):
            {"music": lambda: window.setProperty("modal", "spotify"), "air": lambda: window.setProperty("airOpen", True),
             "chart": lambda: window.setProperty("chartOpen", True), "clock": lambda: window.setProperty("clockOpen", True),
             "settings": lambda: window.setProperty("settingsOpen", True)}[view]()
        elif view == "assist":
            threading.Thread(target=panel.assist, daemon=True).start()
        elif view == "power":
            panel.change(lambda s: s.update(power_menu=True))
        elif view:
            from PyQt5.QtCore import Q_ARG, QMetaObject, QVariant
            QMetaObject.invokeMethod(window, "openPage", Q_ARG(QVariant, view), Q_ARG(QVariant, False))

    def shot():
        image = window.grabWindow()
        if not image.save(args.shot):
            print(f"could not save {args.shot} ({image.width()}x{image.height()})", file=sys.stderr)
        app.quit()

    QTimer.singleShot(1500, show)
    if args.shot:
        QTimer.singleShot(7000 if args.view == "assist" else 4000, shot)
    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
