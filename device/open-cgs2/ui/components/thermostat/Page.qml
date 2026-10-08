import QtQuick 2.14
import "../../qml"

ThermostatBoard {
    property var app
    property var config
    climate: app.st.climate
    staleMinutes: config.stale_after || 0
    outside: app.sensor("outside")
    outsideRh: app.sensor("outside_rh")
    fontDin: app.fontDin
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onSend: app.post(path)
    onTouched: app.pageTouched()
    onClockClicked: app.openClock()
}
