import QtQuick 2.14
import "../../qml"

// The next event from the calendars: a timed one and when it starts, or an all-day one on its
// own days when no timed event starts within 24 hours.
Row {
    id: row
    property var app
    property var config
    property real room
    spacing: 8
    visible: note !== null

    readonly property var note: {
        var list = app.st.calendar || [], best = null, now = app.nowMs;
        for (var i = 0; i < list.length; i++) {
            var e = list[i], start = e.start * 1000;
            if (e.end * 1000 <= now || start > now + (e.all_day ? 0 : 24 * 3600000))
                continue;
            if (best === null || (best.all_day && !e.all_day) || (best.all_day === e.all_day && start < best.start * 1000))
                best = e;
        }
        return best === null ? null : { summary: best.summary, when: best.all_day ? "" : app.untilText(best.start * 1000) };
    }

    Icon {
        id: icon
        anchors.verticalCenter: parent.verticalCenter
        width: 22; height: 22
        kind: "calendar"
        color: "#9aa3b2"
    }
    Text {
        width: Math.max(0, Math.min(implicitWidth, row.room - icon.width - (when.text ? when.width + row.spacing * 2 : row.spacing)))
        elide: Text.ElideRight
        text: row.note ? row.note.summary : ""
        font.family: row.app.fontFamily
        font.pixelSize: 22
        color: "#9aa3b2"
    }
    Text {
        id: when
        visible: text !== ""
        text: row.note ? row.note.when : ""
        font.family: row.app.fontFamily
        font.pixelSize: 22
        color: "#9aa3b2"
    }
}
