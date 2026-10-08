import QtQuick 2.14
import "../../qml"

WeatherBoard {
    property var app
    property var config
    source: app.api + "/forecast"
    fontDin: app.fontDin
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    outside: app.sensor("outside")
    condition: app.st.sensors.sky || ""
    summary: app.st.sensors.rain_soon || ""
    lat: app.st.location ? app.st.location.latitude : 0
    lon: app.st.location ? app.st.location.longitude : 0
    onTouched: app.pageTouched()
    onClockClicked: app.openClock()

    function open(week) {
        show(week);
    }
}
