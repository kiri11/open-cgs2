pragma Singleton
import QtQuick 2.14

// Time formats for the 12- or 24-hour clock in config.json (`units.clock`), set by main.qml.
QtObject {
    property bool clock24: true

    function time(d) {
        return Qt.formatTime(d, clock24 ? "HH:mm" : "h:mm");
    }

    // An "HH:mm" string, such as an alarm's time.
    function hm(s) {
        if (clock24 || !s)
            return s;
        var h = parseInt(s.slice(0, 2)), m = s.slice(3, 5);
        return (h % 12 || 12) + ":" + m + (h < 12 ? " AM" : " PM");
    }
}
