import QtQuick 2.14
import "../../qml"

// Hold to talk to the other units; shown once one is found.
HoldToTalk {
    property var app
    property var config
    property bool compact
    width: 208; height: 64
    readonly property bool shown: app.intercom !== null
    label: held ? "Talking" : "Hold to talk"
    fontFamily: app.fontMedium
    onHeldChanged: app.talkHeld = held
    onSend: app.post(path)
}
