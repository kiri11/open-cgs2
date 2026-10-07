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

    Repeater {
        model: (app.st.groups || []).slice(0, 6)
        Tile {
            label: modelData.name
            icon: modelData.off_only ? "power" : ""
            danger: modelData.off_only === true
            fontFamily: tiles.app.fontMedium
            on: modelData.on
            lightRgb: modelData.rgb || [243, 182, 65]
            brightness: modelData.brightness || 0
            dimmable: modelData.dimmable === true
            onAdjustingChanged: tiles.app.lightAdjusting = adjusting
            onBrightnessRequested: tiles.setBrightness(modelData.id, value)
            enabled: tiles.app.online && modelData.available && (!danger || on)
            onClicked: tiles.app.post("/press/" + modelData.id)
        }
    }
}
