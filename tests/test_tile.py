"""Touch regression checks: run with PyQt5, QT_QPA_PLATFORM=offscreen and QT_QUICK_BACKEND=software."""
import os
from pathlib import Path
import unittest

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("QT_QUICK_BACKEND", "software")

from PyQt5.QtCore import QObject, QPoint, Qt, QUrl
from PyQt5.QtGui import QGuiApplication
from PyQt5.QtQml import QQmlComponent
from PyQt5.QtQuick import QQuickView
from PyQt5.QtTest import QTest

QML = Path(__file__).resolve().parents[1] / "device/open-cgs2/ui/components/lights"
APP = QGuiApplication([])


class LightTouchTests(unittest.TestCase):
    def setUp(self):
        self.view = QQuickView()
        component = QQmlComponent(self.view.engine())
        component.setData(('''
import QtQuick 2.14
import QtQml.Models 2.14
import "%s"
Item {
    width: 720; height: 720
    ListView {
        id: horizontal
        objectName: "horizontal"
        anchors.fill: parent
        orientation: ListView.Horizontal
        interactive: !tile.adjusting
        model: ObjectModel {
            ListView {
                objectName: "vertical"
                width: 720; height: 720
                interactive: !tile.adjusting
                model: ObjectModel {
                    Item {
                        width: 720; height: 720
                        Tile {
                            id: tile
                            objectName: "tile"
                            x: 256; y: 280
                            label: "Kitchen"
                            on: true
                            dimmable: true
                            brightness: 128
                            lightRgb: [255, 0, 0]
                            property int clicks: 0
                            property var sent: []
                            onClicked: clicks++
                            onBrightnessRequested: sent = sent.concat([value])
                        }
                    }
                    Item { width: 720; height: 720 }
                }
            }
            Item { width: 720; height: 720 }
        }
    }
}
''' % QML.as_uri()).encode(), QUrl())
        self.assertFalse(component.isError(), [e.toString() for e in component.errors()])
        self.root = component.create()
        self.assertIsNotNone(self.root)
        self.view.setContent(QUrl(), component, self.root)
        self.view.show()
        QTest.qWait(50)
        self.tile = self.root.findChild(QObject, "tile")

    def tearDown(self):
        self.view.close()
        self.view.deleteLater()
        APP.processEvents()

    def sent(self):
        return self.tile.property("sent").toVariant()

    def press(self, hold=False):
        QTest.mousePress(self.view, Qt.LeftButton, Qt.NoModifier, QPoint(360, 360))
        QTest.qWait(520 if hold else 30)

    def release(self, x=360, y=360):
        QTest.mouseRelease(self.view, Qt.LeftButton, Qt.NoModifier, QPoint(x, y))
        QTest.qWait(30)

    def test_tap_toggles_without_brightness_command(self):
        self.press()
        self.release()
        self.assertEqual(self.tile.property("clicks"), 1)
        self.assertEqual(self.sent(), [])

    def test_hold_drag_updates_outside_tile_without_toggle_or_scroll(self):
        self.press(hold=True)
        self.assertTrue(self.tile.property("adjusting"))
        QTest.mouseMove(self.view, QPoint(360, 120), 20)
        QTest.qWait(180)
        self.assertEqual(self.sent()[-1], 255)
        QTest.mouseMove(self.view, QPoint(360, 600), 20)
        self.release(y=600)
        self.assertEqual(self.sent()[-1], 1)
        self.assertEqual(self.tile.property("clicks"), 0)
        self.assertFalse(self.tile.property("adjusting"))
        self.assertEqual(self.root.findChild(QObject, "vertical").property("contentY"), 0)
        self.assertEqual(self.root.findChild(QObject, "horizontal").property("contentX"), 0)
        self.tile.setProperty("brightness", 1)
        self.assertEqual(self.tile.property("previewBrightness"), -1)

    def test_hold_without_drag_does_not_toggle_or_send(self):
        self.press(hold=True)
        self.release()
        self.assertEqual(self.tile.property("clicks"), 0)
        self.assertEqual(self.sent(), [])

    def test_switch_does_not_enter_brightness_mode(self):
        self.tile.setProperty("dimmable", False)
        self.press(hold=True)
        self.assertFalse(self.tile.property("adjusting"))
        self.release()
        self.assertEqual(self.sent(), [])

    def test_swipe_before_hold_still_scrolls(self):
        self.press()
        for y in (330, 300, 270, 220, 150):
            QTest.mouseMove(self.view, QPoint(360, y), 20)
        self.release(y=150)
        self.assertFalse(self.tile.property("adjusting"))
        self.assertEqual(self.tile.property("clicks"), 0)
        self.assertEqual(self.sent(), [])
        self.assertGreater(self.root.findChild(QObject, "vertical").property("contentY"), 0)

    def test_off_light_drag_starts_at_minimum_and_sends_final_level(self):
        self.tile.setProperty("on", False)
        self.tile.setProperty("brightness", 0)
        self.press(hold=True)
        self.assertEqual(self.tile.property("shownBrightness"), 1)
        QTest.mouseMove(self.view, QPoint(360, 240), 20)
        self.release(y=240)
        self.assertEqual(self.sent()[-1], 128)
        self.assertEqual(self.tile.property("clicks"), 0)

    def test_disconnection_cancels_adjustment(self):
        self.press(hold=True)
        QTest.mouseMove(self.view, QPoint(360, 300), 20)
        self.tile.setProperty("enabled", False)
        self.release(y=300)
        self.assertFalse(self.tile.property("adjusting"))
        self.assertEqual(self.tile.property("previewBrightness"), -1)
        self.assertEqual(self.tile.property("clicks"), 0)

    def test_color_scales_with_brightness_and_text_stays_readable(self):
        color = self.tile.property("fillColor")
        self.assertEqual((color.red(), color.green(), color.blue()), (128, 0, 0))
        self.assertGreater(self.tile.property("ink").red(), 200)
        self.tile.setProperty("brightness", 255)
        self.assertEqual(self.tile.property("fillColor").red(), 255)
        self.tile.setProperty("on", False)
        self.assertEqual(self.tile.property("fillColor").name(), "#1a1f27")


if __name__ == "__main__":
    unittest.main()
