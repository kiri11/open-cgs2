import QtQuick 2.14
import "../../qml"

DoorbellView {
    property var app
    property var config
    doorbell: app.st.doorbell || null
    screenOn: app.screenOn
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onSend: app.post(path)
}
