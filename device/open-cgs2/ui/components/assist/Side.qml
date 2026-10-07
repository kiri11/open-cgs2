import QtQuick 2.14
import "../../qml"

// Tap to talk to Home Assistant's Assist; tap again to stop.
Orb {
    property var app
    property var config
    property bool compact
    width: compact ? 160 : 208; height: width
    label: app.online ? (config.name || "Assist") : app.backend ? "Offline" : "Starting"
    fontFamily: app.fontMedium
    phase: app.phase === "idle" ? "idle" : "listening"
    opacity: app.online ? 1 : 0.45
    onClicked: app.post("/assist")
}
