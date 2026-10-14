import QtQuick 2.14
import "../../qml"

IntercomView {
    property var app
    property var config
    intercom: app.intercom
    localTalking: app.talkHeld
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onHeldChanged: app.replyHeld = held
    onSend: app.post(path)
}
