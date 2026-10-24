import QtQuick 2.14
import "../../qml"

CallView {
    property var app
    property var config
    call: app.st.call || null
    nowMs: app.nowMs
    fontDin: app.fontDin
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onSend: app.post(path)
}
