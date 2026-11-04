import QtQuick 2.14
import "../../qml"

// A tile per light group, up to six: tap to switch, hold and slide to dim.
Grid {
    id: tiles
    property var app
    property var config
    columns: 3
    spacing: 16

    // Serialize each group's requests and coalesce movement while one is in flight.
    property var pending: ({})
    function setBrightness(id, value) {
        var p = pending[id];
        if (p) {
            p.next = value;
            return;
        }
        p = { next: null };
        pending[id] = p;
        var x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE) return;
            var next = p.next;
            delete pending[id];
            if (next !== null && app.online) setBrightness(id, next);
        };
        x.open("POST", app.api + "/brightness/" + id + "/" + value);
        x.send();
    }

    // A count, not the groups: a changed array would rebuild every tile, ending a hold mid-drag.
    Repeater {
        model: Math.min(6, (app.st.groups || []).length)
        Tile {
            readonly property var group: tiles.app.st.groups[index]
            label: group.name
            icon: group.off_only ? "power" : ""
            danger: group.off_only === true
            fontFamily: tiles.app.fontMedium
            on: group.on
            lightRgb: group.rgb || [243, 182, 65]
            brightness: group.brightness || 0
            dimmable: group.dimmable === true
            onAdjustingChanged: tiles.app.lightAdjusting = adjusting
            onBrightnessRequested: tiles.setBrightness(group.id, value)
            enabled: tiles.app.online && group.available && (!danger || on)
            onClicked: tiles.app.post("/press/" + group.id)
        }
    }
}
