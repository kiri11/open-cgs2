import QtQuick 2.14
import "../../qml"

Item {
    id: tile
    property string label
    property string icon
    property string fontFamily
    property bool on: false
    property bool danger: false
    property var lightRgb: [243, 182, 65]
    property int brightness: 255
    property bool dimmable: false
    property bool adjusting: false
    property int previewBrightness: -1
    property int lastSent: -1
    readonly property bool lit: on || previewBrightness > 0
    readonly property int shownBrightness: previewBrightness > 0 ? previewBrightness : brightness > 0 ? brightness : adjusting ? 1 : 255
    readonly property real intensity: shownBrightness / 255
    readonly property color fillColor: !lit ? "#1a1f27" : danger ? "#d9534f"
        : Qt.rgba(lightRgb[0] / 255 * intensity, lightRgb[1] / 255 * intensity, lightRgb[2] / 255 * intensity, 1)
    readonly property color ink: lit && !danger && (0.2126 * fillColor.r + 0.7152 * fillColor.g + 0.0722 * fillColor.b) > 0.45
        ? "#15191f" : "#f1f4f8"
    signal clicked()
    signal brightnessRequested(int value)

    function sendBrightness() {
        if (previewBrightness > 0 && previewBrightness !== lastSent) {
            lastSent = previewBrightness;
            brightnessRequested(previewBrightness);
        }
    }

    function finishAdjustment() {
        if (!adjusting) return;
        sendBrightness();
        adjusting = false;
        if (previewBrightness > 0) settle.restart();
    }

    onBrightnessChanged: {
        if (!adjusting && previewBrightness > 0 && Math.abs(brightness - previewBrightness) <= 2) {
            settle.stop();
            previewBrightness = -1;
        }
    }
    onEnabledChanged: if (!enabled) {
        adjusting = false;
        previewBrightness = -1;
        settle.stop();
    }

    Timer { id: settle; interval: 3000; onTriggered: previewBrightness = -1 }
    Timer { interval: 150; running: tile.adjusting; repeat: true; onTriggered: tile.sendBrightness() }

    width: 208
    height: 160

    Rectangle {
        anchors.fill: parent
        radius: 24
        color: tile.fillColor
        border.width: tile.adjusting ? 3 : tile.lit && !tile.danger ? 1 : 0
        border.color: tile.adjusting ? "#e6ebf1" : "#647080"
        opacity: tile.enabled ? 1 : 0.45
        scale: area.pressed ? 0.95 : 1
        Behavior on color { ColorAnimation { duration: 150 } }
        Behavior on scale { NumberAnimation { duration: 80 } }

        Rectangle {
            x: 22; y: 22
            width: 14; height: 14; radius: 7
            visible: !tile.icon
            color: tile.lit ? tile.ink : "#39414c"
        }

        Icon {
            anchors.centerIn: parent
            width: 72; height: 72
            visible: !!tile.icon
            kind: tile.icon
            color: tile.lit ? tile.ink : "#d7dde5"
        }

        Text {
            x: 22
            width: parent.width - 44
            visible: !tile.icon
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 18
            text: tile.label
            fontSizeMode: Text.HorizontalFit
            minimumPixelSize: 24
            elide: Text.ElideRight
            font.family: tile.fontFamily
            font.pixelSize: 36
            color: tile.lit ? tile.ink : "#d7dde5"
        }

        Text {
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 18
            visible: tile.adjusting || tile.previewBrightness > 0
            text: Math.max(1, Math.round(tile.shownBrightness / 255 * 100)) + "%"
            font.family: tile.fontFamily
            font.pixelSize: 28
            color: tile.ink
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        property real startY: 0
        property int startBrightness: 1
        property bool held: false
        pressAndHoldInterval: 450
        preventStealing: tile.adjusting
        onPressed: held = false
        onPressAndHold: {
            if (!tile.dimmable || tile.danger) return;
            held = true;
            startY = mouse.y;
            startBrightness = tile.previewBrightness > 0 ? tile.previewBrightness : tile.on ? Math.max(1, tile.brightness) : 1;
            tile.lastSent = -1;
            settle.stop();
            tile.adjusting = true;
        }
        onPositionChanged: {
            if (!tile.adjusting) return;
            // A 240 px upward drag spans the full brightness range.
            var value = Math.max(1, Math.min(255, Math.round(startBrightness + (startY - mouse.y) * 254 / 240)));
            if (value !== startBrightness || tile.previewBrightness > 0)
                tile.previewBrightness = value;
        }
        onReleased: tile.finishAdjustment()
        onCanceled: tile.finishAdjustment()
        onClicked: {
            if (held) return;
            settle.stop();
            tile.previewBrightness = -1;
            tile.clicked();
        }
    }
}
