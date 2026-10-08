import QtQuick 2.14
import "../../qml"
import QtQml.Models 2.14

Item {
    id: board
    property string source
    property bool active: false
    property string fontDin
    property string fontFamily
    property string fontMedium
    property var outside: null
    property string condition
    property string summary
    property real lat: 40.72
    property real lon: -74.04
    readonly property bool detail: sections.currentIndex === 1
    signal touched()
    signal clockClicked()

    property var hourly: []
    property var daily: []
    property string raw
    property double now: Date.now()
    property bool failed: false
    property var next: []
    property var rainNext: null
    property var today: null
    property var week: []

    readonly property var current: outside !== null && outside !== undefined && !isNaN(outside) ? Number(outside) : hourly.length ? hourly[0].temp : null
    readonly property var labels: ({
        "sunny": "Sunny", "clear-night": "Clear", "partlycloudy": "Partly cloudy", "cloudy": "Cloudy",
        "rainy": "Rain", "pouring": "Heavy rain", "lightning": "Thunderstorms", "lightning-rainy": "Thunderstorms",
        "snowy": "Snow", "snowy-rainy": "Sleet", "hail": "Hail", "fog": "Fog", "windy": "Windy",
        "windy-variant": "Windy", "exceptional": "Severe weather"
    })
    readonly property var precipitation: ({
        "rainy": true, "pouring": true, "lightning": true, "lightning-rainy": true,
        "snowy": true, "snowy-rainy": true, "hail": true
    })
    readonly property var dayNames: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]

    function fetch() {
        var x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE)
                return;
            try {
                if (x.status !== 200 && x.status !== 0)
                    throw x.status;
                failed = false;
                if (x.responseText === raw)
                    return;
                var f = JSON.parse(x.responseText);
                raw = x.responseText;
                hourly = f.hourly || [];
                daily = f.daily || [];
                rebuild();
            } catch (e) {
                failed = true;
            }
        };
        x.open("GET", source);
        x.send();
    }

    function deg(v) { return v === null || v === undefined ? "—" : Math.round(v) + "°"; }
    function mm(v) { return v > 0 && v < 1 ? v.toFixed(1) : Math.round(v || 0); }
    function level(v) { return v > 0 ? Math.min(1, 0.14 + 0.86 * Math.sqrt(v / 8)) : 0; }

    function sun(ms) {
        var r = Math.PI / 180, d = new Date(ms);
        d.setHours(12, 0, 0, 0);
        var n = Math.round(d.getTime() / 86400000 + 2440587.5 - 2451545 + 0.0008) - lon / 360;
        var m = (357.5291 + 0.98560028 * n) % 360;
        var c = 1.9148 * Math.sin(m * r) + 0.02 * Math.sin(2 * m * r) + 0.0003 * Math.sin(3 * m * r);
        var l = (m + c + 282.9372) % 360;
        var transit = 2451545 + n + 0.0053 * Math.sin(m * r) - 0.0069 * Math.sin(2 * l * r);
        var dec = Math.asin(Math.sin(l * r) * Math.sin(23.4397 * r));
        var cw = (Math.sin(-0.833 * r) - Math.sin(lat * r) * Math.sin(dec)) / (Math.cos(lat * r) * Math.cos(dec));
        var half = Math.acos(Math.max(-1, Math.min(1, cw))) / r / 360;
        return { rise: (transit - half - 2440587.5) * 86400000, set: (transit + half - 2440587.5) * 86400000 };
    }

    function night(ms) {
        var s = sun(ms);
        return ms < s.rise || ms > s.set;
    }

    function rebuild() {
        now = Date.now();
        var offsets = {};
        function local(ms) {
            var day = Math.floor(ms / 86400000);
            if (offsets[day] === undefined)
                offsets[day] = -new Date(day * 86400000 + 43200000).getTimezoneOffset() * 60000;
            return new Date(ms + offsets[day]);
        }
        function key(d) { return d.getUTCFullYear() * 1000 + d.getUTCMonth() * 40 + d.getUTCDate(); }
        var nowLocal = local(now), today_ = key(nowLocal), hours = {}, upcoming = [], rain = null, i;
        for (i = 0; i < hourly.length; i++) {
            var h = hourly[i], t = local(h.t * 1000), k = key(t);
            (hours[k] = hours[k] || []).push({ hour: t.getUTCHours(), rain: h.rain || 0 });
            if (h.t * 1000 > now && upcoming.length < 6)
                upcoming.push(h);
            if (rain === null && precipitation[h.condition] && h.chance >= 40 && (h.t + 3600) * 1000 > now)
                rain = h;
        }
        var days = [], first = null;
        for (i = 0; i < daily.length && days.length < 7; i++) {
            var d = daily[i], start = local(d.t * 1000), dk = key(start);
            if (dk < today_)
                continue;
            if (dk === today_)
                first = d;
            var midnight = Date.UTC(start.getUTCFullYear(), start.getUTCMonth(), start.getUTCDate()) - (start.getTime() - d.t * 1000);
            var s = sun(midnight + 43200000), slots = [];
            for (var n = 0; n < 12; n++) {
                var mid = midnight + (n * 2 + 1) * 3600000;
                slots.push({ rain: null, day: mid > s.rise && mid < s.set, current: dk === today_ && n === Math.floor(nowLocal.getUTCHours() / 2) });
            }
            (hours[dk] || []).forEach(function(e) {
                var slot = slots[Math.floor(e.hour / 2)];
                slot.rain = Math.max(slot.rain || 0, e.rain);
            });
            days.push({ name: dk === today_ ? "Today" : dayNames[start.getUTCDay()], f: d, slots: slots });
        }
        next = upcoming;
        rainNext = rain;
        today = first;
        week = days;
    }

    function show(week) {
        sections.currentIndex = week ? 1 : 0;
    }

    onActiveChanged: {
        if (!active) {
            sections.currentIndex = 0;
            sections.positionViewAtBeginning();
        }
    }
    Timer { interval: 300000; repeat: true; running: true; triggeredOnStart: true; onTriggered: board.fetch() }
    Timer {
        interval: 30000; repeat: true; running: true
        onTriggered: if (Math.floor(Date.now() / 3600000) !== Math.floor(board.now / 3600000)) board.rebuild()
    }

    ListView {
        id: sections
        anchors.fill: parent
        clip: true
        snapMode: ListView.SnapOneItem
        highlightRangeMode: ListView.StrictlyEnforceRange
        highlightMoveDuration: 250
        boundsBehavior: Flickable.StopAtBounds
        model: ObjectModel {
            Item {
                width: 720; height: 620

                Text {
                    id: clock
                    anchors.right: parent.right
                    anchors.rightMargin: 32
                    y: 14
                    function tick() { text = Fmt.time(new Date()); }
                    Timer { interval: 1000; repeat: true; running: board.active; triggeredOnStart: true; onTriggered: clock.tick() }
                    font.family: board.fontDin
                    font.pixelSize: 48
                    color: "#9aa3b2"
                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -16
                        onClicked: board.clockClicked()
                    }
                }

                Text {
                    id: temp
                    x: 28
                    y: 196 - baselineOffset
                    text: board.deg(board.current)
                    font.family: board.fontDin
                    font.pixelSize: 220
                    color: "#f4f6f9"
                }
                WeatherIcon {
                    anchors.left: temp.right
                    anchors.leftMargin: 8
                    y: 70
                    width: 116; height: 116
                    condition: board.condition
                    night: board.night(board.now)
                }
                Text {
                    anchors.right: parent.right
                    anchors.rightMargin: 32
                    y: 112
                    width: 220
                    horizontalAlignment: Text.AlignRight
                    text: board.labels[board.condition] || ""
                    fontSizeMode: Text.HorizontalFit
                    minimumPixelSize: 22
                    font.family: board.fontMedium
                    font.pixelSize: 34
                    color: "#e6ebf1"
                }
                Text {
                    anchors.right: parent.right
                    anchors.rightMargin: 32
                    y: 160
                    visible: board.today !== null
                    text: board.today ? "H " + board.deg(board.today.temp) + "   L " + board.deg(board.today.low) : ""
                    font.family: board.fontFamily
                    font.pixelSize: 28
                    color: "#9aa3b2"
                }
                Text {
                    x: 32; y: 224
                    width: 656
                    text: board.summary
                    elide: Text.ElideRight
                    font.family: board.fontFamily
                    font.pixelSize: 26
                    color: "#9aa3b2"
                }

                Rectangle {
                    x: 32; y: 276
                    width: 656; height: 320
                    radius: 22
                    color: "#161b22"

                    Text {
                        anchors.centerIn: parent
                        visible: board.next.length === 0
                        text: board.failed ? "Can't reach the forecast" : "No forecast yet"
                        font.family: board.fontFamily
                        font.pixelSize: 24
                        color: board.failed ? "#f2c94c" : "#5b6470"
                    }

                    Repeater {
                        model: board.next
                        Item {
                            readonly property var h: modelData
                            x: index * width
                            width: 656 / 6; height: 320

                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 18
                                text: Fmt.time(new Date(h.t * 1000))
                                font.family: board.fontFamily
                                font.pixelSize: 24
                                color: "#9aa3b2"
                            }
                            WeatherIcon {
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 56
                                width: 54; height: 54
                                condition: h.condition
                                night: board.night(h.t * 1000 + 1800000)
                            }
                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 168 - baselineOffset
                                text: board.deg(h.temp)
                                font.family: board.fontDin
                                font.pixelSize: 60
                                color: "#f4f6f9"
                            }
                            Rectangle {
                                id: slot
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 184
                                width: 26; height: 70
                                radius: 7
                                color: "#232a34"
                                Rectangle {
                                    anchors.bottom: parent.bottom
                                    width: parent.width
                                    height: Math.max(width / 2, parent.height * board.level(h.rain))
                                    visible: h.rain > 0
                                    radius: 7
                                    color: "#3ec6ff"
                                }
                            }
                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 262
                                text: Math.round(h.chance || 0) + "%"
                                font.family: board.fontMedium
                                font.pixelSize: 22
                                color: h.chance > 0 ? "#3ec6ff" : "#5b6470"
                            }
                            Text {
                                anchors.horizontalCenter: parent.horizontalCenter
                                y: 290
                                visible: h.rain > 0
                                text: board.mm(h.rain) + " mm"
                                font.family: board.fontFamily
                                font.pixelSize: 18
                                color: "#7d8796"
                            }
                        }
                    }
                }
            }

            Item {
                width: 720; height: 720

                Item {
                    width: 720; height: 88

                    Text {
                        x: 32; y: 26
                        text: "Next 7 days"
                        font.family: board.fontMedium
                        font.pixelSize: 32
                        color: "#e6ebf1"
                    }
                    Icon {
                        x: 650; y: 30
                        width: 32; height: 32
                        kind: "down"
                        color: "#9aa3b2"
                        rotation: board.detail ? 180 : 0
                        Behavior on rotation { NumberAnimation { duration: 250 } }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: sections.currentIndex = board.detail ? 0 : 1
                    }
                }

                Repeater {
                    model: board.week
                    Item {
                        readonly property var d: modelData
                        x: 32; y: 88 + index * 88
                        width: 656; height: 88

                        Rectangle {
                            width: parent.width; height: 1
                            color: "#232a34"
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: d.name
                            font.family: board.fontMedium
                            font.pixelSize: 32
                            color: "#e6ebf1"
                        }
                        WeatherIcon {
                            x: 104
                            anchors.verticalCenter: parent.verticalCenter
                            width: 46; height: 46
                            condition: d.f.condition
                        }
                        Text {
                            x: 160; width: 56
                            anchors.verticalCenter: parent.verticalCenter
                            horizontalAlignment: Text.AlignRight
                            text: board.deg(d.f.temp)
                            font.family: board.fontDin
                            font.pixelSize: 44
                            color: "#f4f6f9"
                        }
                        Text {
                            x: 220; width: 56
                            anchors.verticalCenter: parent.verticalCenter
                            horizontalAlignment: Text.AlignRight
                            text: board.deg(d.f.low)
                            font.family: board.fontDin
                            font.pixelSize: 44
                            color: "#6b7482"
                        }
                        Row {
                            x: 292
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 5
                            Repeater {
                                model: d.slots
                                Rectangle {
                                    width: 9; height: 56
                                    radius: 4
                                    color: modelData.day ? "#2c333e" : "#191e26"
                                    border.width: modelData.current ? 2 : 0
                                    border.color: "#f4f6f9"
                                    Rectangle {
                                        anchors.bottom: parent.bottom
                                        width: parent.width
                                        height: Math.max(6, parent.height * board.level(modelData.rain))
                                        visible: modelData.rain > 0
                                        radius: 4
                                        color: "#3ec6ff"
                                    }
                                }
                            }
                        }
                        Text {
                            anchors.right: parent.right
                            anchors.rightMargin: 92
                            anchors.verticalCenter: parent.verticalCenter
                            text: board.mm(d.f.rain) + " mm"
                            font.family: board.fontFamily
                            font.pixelSize: 26
                            color: d.f.rain > 0 ? "#aab3c0" : "#5b6470"
                        }
                        Row {
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 2
                            WeatherIcon {
                                anchors.verticalCenter: parent.verticalCenter
                                width: 24; height: 24
                                visible: d.f.chance > 0
                                condition: "drop"
                            }
                            Text {
                                text: Math.round(d.f.chance || 0) + "%"
                                font.family: board.fontMedium
                                font.pixelSize: 28
                                color: d.f.chance > 0 ? "#f4f6f9" : "#5b6470"
                            }
                        }
                    }
                }
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        z: 1
        onPressed: { board.touched(); mouse.accepted = false; }
    }
}
