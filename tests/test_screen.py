"""The whole screen against the emulator's made-up state: every component loads without QML
errors and lands where config.json and the manifests put it."""
import json
import os
import sys
import threading
import unittest
from pathlib import Path

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("QT_QUICK_BACKEND", "software")

from PyQt5.QtCore import QUrl
from PyQt5.QtGui import QGuiApplication
from PyQt5.QtQml import QQmlApplicationEngine, QQmlComponent
from PyQt5.QtQuick import QQuickWindow  # noqa: F401, so that the root object comes back as one
from PyQt5.QtTest import QTest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "emulator"))
import emulator  # noqa: E402

APP = QGuiApplication([])


class ScreenTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        config = json.loads((ROOT / "emulator/demo.json").read_text(encoding="utf-8"))
        config["pages"] = {"down": ["thermostat"]}
        cls.panel = emulator.Panel(config)
        cls.server = emulator.Server(("127.0.0.1", emulator.PORT), emulator.handler(cls.panel))
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def setUp(self):
        self.engine = QQmlApplicationEngine()
        self.warnings = []
        self.engine.warnings.connect(lambda ws: self.warnings.extend(w.toString() for w in ws if "FontLoader" not in w.toString()))
        self.engine.load(QUrl.fromLocalFile(str(ROOT / "device/open-cgs2/ui/qml/main.qml")))
        self.window = self.engine.rootObjects()[0]
        QTest.qWait(1500)

    def tearDown(self):
        self.window.close()
        self.engine.deleteLater()
        APP.processEvents()

    def test_components_load_without_errors(self):
        self.assertEqual(self.warnings, [])
        names = [c["name"] for c in self.window.property("components").toVariant()]
        self.assertIn("weather", names)
        self.assertNotIn("calls", names)
        pages = self.window.property("pageItems").toVariant()
        self.assertEqual(sorted(pages), ["path", "thermostat", "weather"])

    def test_pages_follow_config_then_manifests(self):
        placed = self.window.property("placed").toVariant()
        where = {d: [c["name"] for c in placed[d]] for d in placed}
        self.assertEqual(where, {"left": ["path"], "right": ["weather"], "up": [], "down": ["thermostat"]})
        self.assertEqual(self.window.property("homeColumn"), 1)
        self.assertEqual(self.window.property("homeRow"), 0)

    def test_twelve_hour_times(self):
        qml = ('import QtQuick 2.14\nimport "%s"\nQtObject { property string a: Fmt.hm("19:05"); property string b: Fmt.hm("00:30");'
               ' Component.onCompleted: Fmt.clock24 = false }' % (ROOT / "device/open-cgs2/ui/qml").as_uri())
        component = QQmlComponent(self.engine)
        component.setData(qml.encode(), QUrl())
        o = component.create()
        self.assertEqual((o.property("a"), o.property("b")), ("7:05 PM", "12:30 AM"))

if __name__ == "__main__":
    unittest.main()
