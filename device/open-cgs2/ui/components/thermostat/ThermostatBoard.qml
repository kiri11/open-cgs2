import QtQuick 2.14
import "../../qml"

Item {
    id: board
    property var climate
    property bool active: false
    property var outside: null
    property var outsideRh: null
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal send(string path)
    signal touched()
    signal clockClicked()

    readonly property color heatColor: "#ff8a3d"
    readonly property color coolColor: "#3fa9ff"
    readonly property color ecoColor: "#4cd07d"
    readonly property real gap: 1.5

    readonly property var c: climate || ({})
    readonly property bool available: !!c.mode && c.mode !== "unavailable" && c.mode !== "unknown"
    readonly property string mode: pMode || c.mode || "off"
    readonly property bool eco: c.preset_mode === "eco"
    readonly property bool fanOn: (pFan || c.fan_mode) === "on"
    readonly property bool switching: pMode !== "" && pMode !== c.mode
    readonly property real target: switching ? NaN : isNaN(pTarget) ? num(c.temperature) : pTarget
    readonly property real low: switching ? NaN : isNaN(pLow) ? num(c.target_temp_low) : pLow
    readonly property real high: switching ? NaN : isNaN(pHigh) ? num(c.target_temp_high) : pHigh
    readonly property real current: num(c.current_temperature)
    readonly property real minTemp: num(c.min_temp) || 10
    readonly property real maxTemp: num(c.max_temp) || 32
    readonly property bool dual: mode === "heat_cool"
    readonly property bool adjustable: available && !eco && !switching && (mode === "heat" || mode === "cool" || dual)
    readonly property color accent: !available || mode === "off" ? "#5b6470" : eco ? ecoColor : mode === "cool" || (dual && editing === "high") ? coolColor : heatColor
    property string editing: "low"
    property double now: Date.now()
    // Some thermostats report to HA only now and then; after this many minutes without an update
    // the values dim (0 never dims them).
    property int staleMinutes: 0
    readonly property bool stale: staleMinutes > 0 && available && c.updated > 0 && now / 1000 - c.updated > staleMinutes * 60
    readonly property real fade: stale ? 0.35 : 1

    property string pMode: ""
    property string pFan: ""
    property real pTarget: NaN
    property real pLow: NaN
    property real pHigh: NaN

    function num(v) {
        var n = parseFloat(v);
        return isNaN(n) ? NaN : n;
    }
    function fmt(v) {
        return isNaN(v) ? "—" : v % 1 === 0 ? v.toFixed(0) : v.toFixed(1);
    }
    function clamp(v, a, b) {
        return Math.max(a, Math.min(b, Math.round(v * 2) / 2));
    }
    function same(a, b) {
        return Math.abs(num(a) - b) < 0.05;
    }

    function set(which, v) {
        if (!adjustable)
            return;
        if (which === "low")
            pLow = clamp(v, minTemp, high - gap);
        else if (which === "high")
            pHigh = clamp(v, low + gap, maxTemp);
        else
            pTarget = clamp(v, minTemp, maxTemp);
        commit.restart();
        touched();
    }
    function step(d) {
        if (dual)
            set(editing, (editing === "low" ? low : high) + d);
        else
            set("target", target + d);
    }
    function submit(path) {
        send(path);
        settle.restart();
        touched();
    }
    function setMode(m) {
        if (!available || m === mode)
            return;
        commit.stop();
        pTarget = pLow = pHigh = NaN;
        pMode = m;
        submit("/climate/mode/" + m);
    }

    onCChanged: {
        if (pMode && c.mode === pMode) pMode = "";
        if (pFan && c.fan_mode === pFan) pFan = "";
        if (!commit.running) {
            if (same(c.temperature, pTarget)) pTarget = NaN;
            if (same(c.target_temp_low, pLow)) pLow = NaN;
            if (same(c.target_temp_high, pHigh)) pHigh = NaN;
        }
    }
    onActiveChanged: if (!active) editing = "low"; else now = Date.now()
    Timer { interval: 60000; repeat: true; running: board.active; onTriggered: board.now = Date.now() }

    Timer {
        id: commit
        interval: 1200
        onTriggered: board.submit(board.dual ? "/climate/range/" + board.low + "/" + board.high : "/climate/temp/" + board.target)
    }
    Timer {
        id: settle
        interval: 20000
        onTriggered: {
            board.pMode = board.pFan = "";
            board.pTarget = board.pLow = board.pHigh = NaN;
        }
    }

    MouseArea {
        anchors.fill: parent
        z: 1
        onPressed: { board.touched(); mouse.accepted = false; }
    }

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

    Repeater {
        model: [
            { label: "Inside", temp: isNaN(board.current) ? null : board.current, rh: isNaN(board.num(board.c.current_humidity)) ? null : board.num(board.c.current_humidity), x: 32, color: "#f4f6f9", fade: board.fade },
            { label: "Outside", temp: board.outside, rh: board.outsideRh, x: 262, color: "#aab3c0", fade: 1 }
        ]
        Item {
            x: modelData.x; y: 18
            opacity: modelData.fade
            Text {
                text: modelData.label
                font.family: board.fontFamily
                font.pixelSize: 22
                color: "#7d8796"
            }
            Text {
                id: reading
                y: 112 - baselineOffset
                text: modelData.temp === null ? "—" : modelData.temp.toFixed(1) + "°"
                font.family: board.fontDin
                font.pixelSize: 88
                color: modelData.color
            }
            Text {
                anchors.left: reading.right
                anchors.leftMargin: 10
                anchors.baseline: reading.baseline
                visible: modelData.rh !== null
                text: Math.round(modelData.rh) + "%"
                font.family: board.fontFamily
                font.pixelSize: 24
                color: "#6b7482"
            }
        }
    }

    Canvas {
        id: dial
        readonly property real cx: 360
        readonly property real cy: 388
        readonly property real r: 190
        readonly property string key: [board.available, board.mode, board.eco, board.target, board.low, board.high, board.current, board.editing, board.minTemp, board.maxTemp].join()
        x: 0; y: 0
        width: 720; height: 720
        opacity: board.fade
        onKeyChanged: requestPaint()

        function angle(v) {
            return Math.PI * 0.75 + Math.max(0, Math.min(1, (v - board.minTemp) / (board.maxTemp - board.minTemp))) * Math.PI * 1.5;
        }
        function value(px, py) {
            var t = Math.atan2(py - cy, px - cx) - Math.PI * 0.75;
            while (t < 0) t += Math.PI * 2;
            if (t > Math.PI * 1.5)
                t = t < Math.PI * 1.75 ? Math.PI * 1.5 : 0;
            return board.minTemp + t / (Math.PI * 1.5) * (board.maxTemp - board.minTemp);
        }

        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            ctx.lineCap = "round";
            function arc(a, b, color, width) {
                if (b - a < 0.001)
                    return;
                ctx.strokeStyle = color;
                ctx.lineWidth = width;
                ctx.beginPath();
                ctx.arc(cx, cy, r, a, b);
                ctx.stroke();
            }
            function knob(v, color, size) {
                var a = angle(v);
                ctx.fillStyle = "#0d1015";
                ctx.beginPath();
                ctx.arc(cx + Math.cos(a) * r, cy + Math.sin(a) * r, size + 5, 0, Math.PI * 2);
                ctx.fill();
                ctx.fillStyle = color;
                ctx.beginPath();
                ctx.arc(cx + Math.cos(a) * r, cy + Math.sin(a) * r, size, 0, Math.PI * 2);
                ctx.fill();
                ctx.fillStyle = "#ffffff";
                ctx.beginPath();
                ctx.arc(cx + Math.cos(a) * r, cy + Math.sin(a) * r, size * 0.4, 0, Math.PI * 2);
                ctx.fill();
            }

            ctx.fillStyle = "#2a313c";
            for (var t = Math.ceil(board.minTemp); t <= board.maxTemp; t++) {
                var ta = angle(t), big = t % 5 === 0, tr = r - 40;
                ctx.beginPath();
                ctx.arc(cx + Math.cos(ta) * tr, cy + Math.sin(ta) * tr, big ? 3.5 : 2, 0, Math.PI * 2);
                ctx.fill();
            }

            var start = Math.PI * 0.75, end = Math.PI * 2.25;
            arc(start, end, "#1b212b", 22);
            var on = board.available && board.mode !== "off";
            var heat = board.eco ? "#2f6b48" : board.heatColor, cool = board.eco ? "#2f6b48" : board.coolColor;
            if (on && board.dual && !isNaN(board.low) && !isNaN(board.high)) {
                arc(start, angle(board.low), heat, 22);
                arc(angle(board.high), end, cool, 22);
            } else if (on && board.mode === "heat" && !isNaN(board.target)) {
                arc(start, angle(board.target), heat, 22);
            } else if (on && board.mode === "cool" && !isNaN(board.target)) {
                arc(angle(board.target), end, cool, 22);
            }

            if (!isNaN(board.current)) {
                var ca = angle(board.current);
                ctx.strokeStyle = "#ffffff";
                ctx.lineWidth = 5;
                ctx.beginPath();
                ctx.moveTo(cx + Math.cos(ca) * (r - 17), cy + Math.sin(ca) * (r - 17));
                ctx.lineTo(cx + Math.cos(ca) * (r + 17), cy + Math.sin(ca) * (r + 17));
                ctx.stroke();
            }

            if (board.adjustable) {
                if (board.dual) {
                    knob(board.editing === "low" ? board.high : board.low, board.editing === "low" ? board.coolColor : board.heatColor, 15);
                    knob(board.editing === "low" ? board.low : board.high, board.editing === "low" ? board.heatColor : board.coolColor, 22);
                } else if (!isNaN(board.target)) {
                    knob(board.target, board.accent, 22);
                }
            }
        }

        MouseArea {
            anchors.fill: parent
            preventStealing: true
            enabled: board.adjustable
            property string which: ""
            onPressed: {
                var d = Math.sqrt(Math.pow(mouse.x - dial.cx, 2) + Math.pow(mouse.y - dial.cy, 2));
                if (Math.abs(d - dial.r) > 48 || mouse.y > dial.cy + dial.r * 0.62 && Math.abs(mouse.x - dial.cx) < dial.r * 0.62) {
                    mouse.accepted = false;
                    return;
                }
                var v = dial.value(mouse.x, mouse.y);
                which = !board.dual ? "target" : Math.abs(v - board.low) <= Math.abs(v - board.high) ? "low" : "high";
                if (board.dual)
                    board.editing = which;
                board.set(which, v);
            }
            onPositionChanged: board.set(which, dial.value(mouse.x, mouse.y))
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 262
        visible: board.available && board.mode !== "off"
        opacity: board.fade
        text: {
            var a = board.c.hvac_action;
            var s = a === "heating" ? "Heating" : a === "cooling" ? "Cooling" : a === "fan" ? "Fan only" : "Idle";
            return board.eco ? "Eco · " + s : s;
        }
        font.family: board.fontMedium
        font.pixelSize: 26
        font.letterSpacing: 2
        font.capitalization: Font.AllUppercase
        color: board.c.hvac_action === "heating" ? board.heatColor : board.c.hvac_action === "cooling" ? board.coolColor : board.eco ? board.ecoColor : "#8a94a3"
    }

    Item {
        id: single
        anchors.horizontalCenter: parent.horizontalCenter
        y: 300
        width: big.width + deg.width; height: 150
        visible: !board.dual
        opacity: board.fade

        Text {
            id: big
            y: 136 - baselineOffset
            text: !board.available ? "—" : board.mode === "off" ? "Off" : board.fmt(board.target)
            font.family: board.fontDin
            font.pixelSize: board.available && board.mode !== "off" ? 168 : 120
            color: board.available && board.mode !== "off" ? "#f4f6f9" : "#5b6470"
        }
        Text {
            id: deg
            anchors.left: big.right
            y: big.y + 26
            visible: board.available && board.mode !== "off" && !isNaN(board.target)
            text: "°"
            font.family: board.fontDin
            font.pixelSize: 72
            color: "#9aa3b2"
        }
    }

    Row {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 304
        spacing: 16
        visible: board.dual
        opacity: board.fade

        Repeater {
            model: [
                { which: "low", label: "Heat to", color: board.heatColor },
                { which: "high", label: "Cool to", color: board.coolColor }
            ]
            Item {
                readonly property bool selected: board.editing === modelData.which || board.eco
                width: Math.max(setpoint.width, 110); height: 160
                Text {
                    id: setpoint
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 112 - baselineOffset
                    readonly property real v: modelData.which === "low" ? board.low : board.high
                    text: board.fmt(v) + (isNaN(v) ? "" : "°")
                    font.family: board.fontDin
                    font.pixelSize: 88
                    color: parent.selected ? "#f4f6f9" : "#5b6470"
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: 122
                    text: modelData.label
                    font.family: board.fontMedium
                    font.pixelSize: 22
                    color: board.eco ? board.ecoColor : parent.selected ? modelData.color : "#5b6470"
                }
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -10
                    onClicked: { board.editing = modelData.which; board.touched(); }
                }
            }
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 546
        visible: board.stale
        text: "Not updated since " + Fmt.time(new Date(board.c.updated * 1000))
        font.family: board.fontFamily
        font.pixelSize: 22
        color: "#f2c94c"
    }

    Repeater {
        model: [{ d: -0.5, icon: "minus", x: 32 }, { d: 0.5, icon: "plus", x: 584 }]
        Rectangle {
            x: modelData.x; y: dial.cy - 52
            width: 104; height: 104; radius: 52
            color: stepArea.pressed ? "#262d38" : "#1a1f27"
            opacity: board.adjustable ? 1 : 0.35
            scale: stepArea.pressed ? 0.93 : 1
            Behavior on scale { NumberAnimation { duration: 80 } }
            Icon {
                anchors.centerIn: parent
                width: 44; height: 44
                kind: modelData.icon
                color: "#e6ebf1"
            }
            MouseArea {
                id: stepArea
                anchors.fill: parent
                anchors.margins: -12
                enabled: board.adjustable
                onClicked: board.step(modelData.d)
                onPressAndHold: repeat.start()
                onReleased: repeat.stop()
                onCanceled: repeat.stop()
                Timer { id: repeat; interval: 180; repeat: true; onTriggered: board.step(modelData.d) }
            }
        }
    }

    Rectangle {
        id: modes
        x: 32; y: 600
        width: 552; height: 88
        radius: 24
        color: "#161b22"

        Rectangle {
            id: thumb
            readonly property int index: Math.max(0, ["off", "heat", "cool", "heat_cool"].indexOf(board.mode))
            x: 4 + index * 136; y: 4
            width: 136; height: 80
            radius: 20
            visible: board.available
            opacity: board.fade
            color: board.mode === "heat" ? board.heatColor : board.mode === "cool" ? board.coolColor : board.mode === "heat_cool" ? "#8a6cff" : "#3a424e"
            Behavior on x { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: 160 } }
        }

        Row {
            x: 4; y: 4
            Repeater {
                model: [
                    { mode: "off", label: "Off", icon: "power" },
                    { mode: "heat", label: "Heat", icon: "flame" },
                    { mode: "cool", label: "Cool", icon: "snow" },
                    { mode: "heat_cool", label: "Heat·Cool", icon: "" }
                ]
                Item {
                    readonly property bool selected: board.available && board.mode === modelData.mode
                    readonly property bool supported: !board.c.hvac_modes || board.c.hvac_modes.indexOf(modelData.mode) >= 0
                    width: 136; height: 80
                    opacity: board.available && supported ? 1 : 0.35

                    Row {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: 10
                        spacing: -6
                        Icon {
                            width: 34; height: 34
                            kind: modelData.icon || "flame"
                            color: parent.parent.selected ? "#ffffff" : "#aab3c0"
                        }
                        Icon {
                            width: 34; height: 34
                            visible: !modelData.icon
                            kind: "snow"
                            color: parent.parent.selected ? "#ffffff" : "#aab3c0"
                        }
                    }
                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: 48
                        text: modelData.label
                        font.family: board.fontMedium
                        font.pixelSize: 20
                        color: parent.selected ? "#ffffff" : "#aab3c0"
                    }
                    MouseArea {
                        anchors.fill: parent
                        enabled: board.available && parent.supported
                        onClicked: board.setMode(modelData.mode)
                    }
                }
            }
        }
    }

    Rectangle {
        readonly property bool on: board.fanOn
        x: 600; y: 600
        width: 88; height: 88
        radius: 24
        color: on ? "#2fb7c9" : "#161b22"
        opacity: board.available && board.c.fan_mode !== undefined ? 1 : 0.35
        scale: fanArea.pressed ? 0.94 : 1
        Behavior on color { ColorAnimation { duration: 150 } }
        Behavior on scale { NumberAnimation { duration: 80 } }

        Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 14
            width: 34; height: 34
            kind: "fan"
            color: parent.on ? "#0d1015" : "#aab3c0"
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 52
            text: "Fan"
            font.family: board.fontMedium
            font.pixelSize: 20
            color: parent.on ? "#0d1015" : "#aab3c0"
        }
        MouseArea {
            id: fanArea
            anchors.fill: parent
            enabled: board.available && board.c.fan_mode !== undefined
            onClicked: {
                board.pFan = board.fanOn ? "off" : "on";
                board.submit("/climate/fan/" + board.pFan);
            }
        }
    }
}
