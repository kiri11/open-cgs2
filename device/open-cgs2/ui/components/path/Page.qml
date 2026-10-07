import QtQuick 2.14
import "../../qml"

// Next PATH trains from `station` (a RidePATH code such as "GRV"), fetched through the panel.
PathBoard {
    property var app
    property var config
    station: config.station || "GRV"
    source: config.source || "https://www.panynj.gov/bin/portauthority/ridepath.json"
    hideUnder: config.hide_under_minutes || 0
    fetcher: app.fetch
    fontDin: app.fontDin
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onTouched: app.pageTouched()
    onClockClicked: app.openClock()
}
