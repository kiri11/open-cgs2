import QtQuick 2.14
import "../../qml"

// Rain, a thunderstorm or snow at a 40% chance or more within 24 hours: that hour's icon and
// when it starts. Tapping it opens the forecast.
Item {
    id: note
    property var app
    property var config
    property real room
    width: row.width; height: row.height
    visible: rain !== null
    opacity: area.pressed ? 0.6 : 1

    readonly property var board: app.page("weather")
    readonly property var rain: {
        if (!board)
            return null;
        var sky = app.st.sensors.sky || "", h = board.rainNext;
        if (board.precipitation[sky])
            return { condition: sky, text: "now", soon: true };
        if (h === null)
            return null;
        var ms = h.t * 1000 - app.nowMs;
        return ms > 24 * 3600000 ? null : { condition: h.condition, text: app.untilText(h.t * 1000), soon: ms <= 6 * 3600000 };
    }

    Row {
        id: row
        spacing: 6
        WeatherIcon {
            anchors.verticalCenter: parent.verticalCenter
            width: 26; height: 26
            condition: note.rain ? note.rain.condition : ""
        }
        Text {
            text: note.rain ? note.rain.text : ""
            font.family: note.app.fontFamily
            font.pixelSize: 22
            color: "#9aa3b2"
        }
    }
    MouseArea {
        id: area
        x: -12; y: -12
        width: note.width + 24; height: note.height + 24
        onClicked: note.app.openPage("weather", !note.rain.soon)
    }
}
