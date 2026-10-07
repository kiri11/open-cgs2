import QtQuick 2.14
import "../../qml"

// The next trains from one station of the RidePATH feed: up to two destinations towards New York
// above the line and two towards New Jersey below it, three departures each.
Item {
    id: board
    property string station
    property string source
    // Trains leaving sooner than this many minutes are left out, for the walk to the station.
    property int hideUnder: 0
    // fetcher(url, done) passes the parsed JSON, or null.
    property var fetcher
    property bool active: false
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal touched()
    signal clockClicked()

    readonly property var slots: [76, 228, 390, 542]

    property var trains: []
    property double received: 0
    property double now: Date.now()
    property bool failed: false
    readonly property bool loaded: received > 0
    readonly property bool stale: loaded && now - received > 90000
    // Destinations as cards: two towards New York, then two towards New Jersey.
    readonly property var lines: {
        var out = [], seen = {};
        for (var dir = 0; dir < 2; dir++) {
            var n = 0;
            for (var i = 0; i < trains.length && n < 2; i++) {
                var t = trains[i];
                if (t.dir !== dir || seen[t.headsign])
                    continue;
                seen[t.headsign] = true;
                out.push({ headsign: t.headsign, label: t.headsign, color: t.color, y: slots[dir * 2 + n] });
                n++;
            }
        }
        return out;
    }

    function fetch() {
        fetcher(source, function(v) {
            var list = [];
            try {
                var results = v.results;
                for (var i = 0; i < results.length; i++) {
                    if (results[i].consideredStation !== station)
                        continue;
                    var dests = results[i].destinations;
                    for (var j = 0; j < dests.length; j++)
                        for (var k = 0; k < dests[j].messages.length; k++) {
                            var m = dests[j].messages[k];
                            list.push({ headsign: m.headSign.replace(/ via .*/, ""), via: / via /.test(m.headSign), dir: dests[j].label === "ToNY" ? 0 : 1,
                                        color: "#" + m.lineColor.split(",")[0], "in": parseInt(m.secondsToArrival) });
                        }
                }
                list.sort(function(a, b) { return a["in"] - b["in"]; });
                trains = list;
                received = Date.now();
                now = received;
                failed = false;
            } catch (e) {
                failed = true;
            }
        });
    }

    function departures(headsign) {
        var out = [], age = (now - received) / 1000;
        for (var i = 0; i < trains.length && out.length < 3; i++) {
            var t = trains[i], left = t["in"] - age;
            if (t.headsign !== headsign || left < hideUnder * 60)
                continue;
            out.push({ min: Math.floor(left / 60), at: Fmt.time(new Date(now + left * 1000)), via: t.via });
        }
        return out;
    }

    onActiveChanged: if (active) { now = Date.now(); fetch(); }
    Timer { interval: 15000; repeat: true; running: board.active; onTriggered: board.fetch() }
    Timer { interval: 5000; repeat: true; running: board.active; onTriggered: board.now = Date.now() }

    MouseArea {
        anchors.fill: parent
        onPressed: { board.touched(); mouse.accepted = false; }
    }

    Text {
        anchors.right: parent.right
        anchors.rightMargin: 32
        y: 14
        text: Fmt.time(new Date(board.now))
        font.family: board.fontDin
        font.pixelSize: 48
        color: "#9aa3b2"
        MouseArea {
            anchors.fill: parent
            anchors.margins: -16
            onClicked: board.clockClicked()
        }
    }

    Rectangle {
        x: 32; y: 378
        visible: board.lines.length > 0
        width: 656; height: 2
        color: "#8a94a3"
    }

    Repeater {
        model: board.lines
        Rectangle {
            id: card
            readonly property var line: modelData
            readonly property var times: { board.now; board.trains; return board.departures(line.headsign); }
            x: 32; y: line.y
            width: 656; height: 140
            radius: 22
            color: "#161b22"

            Rectangle {
                x: 24
                anchors.verticalCenter: parent.verticalCenter
                width: 8; height: 76; radius: 4
                color: card.line.color
            }
            Text {
                x: 50
                width: 210
                anchors.verticalCenter: parent.verticalCenter
                text: card.line.label + (card.times.length && card.times[0].via ? " (via)" : "")
                fontSizeMode: Text.HorizontalFit
                minimumPixelSize: 22
                font.family: board.fontMedium
                font.pixelSize: 36
                color: "#e6ebf1"
            }
            Text {
                anchors.right: parent.right
                anchors.rightMargin: 28
                anchors.verticalCenter: parent.verticalCenter
                visible: card.times.length === 0 && (board.loaded || board.failed)
                text: board.loaded ? "No live times yet" : "Can't reach PATH"
                font.family: board.fontFamily
                font.pixelSize: 24
                color: board.loaded ? "#5b6470" : "#f2c94c"
            }
            Repeater {
                model: card.times
                Item {
                    readonly property bool first: index === 0
                    x: first ? 290 : 312 + index * 106
                    width: first ? 116 : 100
                    height: card.height
                    opacity: board.stale ? 0.5 : 1

                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: 90 - baselineOffset
                        text: modelData.min
                        font.family: board.fontDin
                        font.pixelSize: first ? 112 : 84
                        color: modelData.min < board.hideUnder + 2 ? "#eb5757" : modelData.min < board.hideUnder + 4 ? "#f2994a" : first ? "#f4f6f9" : "#aab3c0"
                    }
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: 100
                        text: modelData.at
                        font.family: board.fontFamily
                        font.pixelSize: 20
                        color: "#6b7482"
                    }
                }
            }
        }
    }

    Text {
        anchors.centerIn: parent
        visible: board.lines.length === 0
        text: board.failed ? "Can't reach PATH" : board.loaded ? "No trains listed for " + board.station : "Loading…"
        font.family: board.fontFamily
        font.pixelSize: 28
        color: board.failed ? "#f2c94c" : "#5b6470"
    }
}
