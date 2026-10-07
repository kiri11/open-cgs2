import QtQuick 2.14

Rectangle {
    id: view
    property string key
    property string api
    property string current
    property color currentColor: "#e6ebf1"
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal close()
    signal pick(string key)

    readonly property var climateTabs: [
        { key: "co2", label: "CO<sub>2</sub>" },
        { key: "temp", label: "Temp" },
        { key: "rh", label: "Humidity" }
    ]
    readonly property var airTabs: [
        { key: "tvoc", label: "eTVOC" },
        { key: "pm25", label: "PM2.5" },
        { key: "pm10", label: "PM10" }
    ]
    property var tabs: climateTabs
    readonly property var ranges: [
        { key: "3h", label: "3H" },
        { key: "1d", label: "1D" },
        { key: "1w", label: "1W" },
        { key: "1m", label: "1M" },
        { key: "1y", label: "1Y" },
        { key: "all", label: "All" }
    ]
    property string range: "3h"
    readonly property var monthNames: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    readonly property var dayNames: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    readonly property var meta: ({
        co2: { unit: "ppm", span: 200, digits: 0, suffix: "", bands: [[0, 1000, "#4cd07d"], [1000, 1400, "#f2c94c"], [1400, 100000, "#eb5757"]] },
        tvoc: { unit: "index", span: 40, digits: 0, suffix: "", bands: [[0, 66, "#4cd07d"], [66, 92, "#f2c94c"], [92, 137, "#f2994a"], [137, 1000, "#eb5757"]] },
        pm25: { unit: "µg/m³", span: 10, digits: 0, suffix: "", bands: [[0, 9, "#4cd07d"], [9, 35.4, "#f2c94c"], [35.4, 55.4, "#f2994a"], [55.4, 100000, "#eb5757"]] },
        pm10: { unit: "µg/m³", span: 20, digits: 0, suffix: "", bands: [[0, 54, "#4cd07d"], [54, 154, "#f2c94c"], [154, 254, "#f2994a"], [254, 100000, "#eb5757"]] },
        temp: { unit: "", span: 3, digits: 1, suffix: "°", bands: [] },
        rh: { unit: "", span: 10, digits: 0, suffix: "%", bands: [[40, 60, "#4cd07d"]] }
    })
    readonly property var palette: ["#5aa9e6", "#c792ea", "#f2994a"]
    readonly property color hereColor: "#f4f6f9"
    readonly property color outsideColor: "#8a94a3"

    readonly property real plotX: 84
    readonly property real plotY: 222
    readonly property real plotW: 600
    readonly property real plotH: 296

    property var hist: null
    property var lines: []
    property real lo: 0
    property real hi: 1
    property var ticks: []
    property var xticks: []
    property int count: 0
    property int cursor: -1
    readonly property bool ready: hist !== null && hist.key === key && hist.range === range

    color: "#0b0d12"

    function fetch() {
        var k = key, r = range, x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE || x.status !== 200 || k !== key || r !== range)
                return;
            try {
                var v = JSON.parse(x.responseText);
                v.key = k;
                v.range = r;
                load(v);
            } catch (e) {}
        };
        x.open("GET", api + "/history/" + k + "?range=" + r);
        x.send();
    }

    function niceStep(x) {
        var p = Math.pow(10, Math.floor(Math.log(x) / Math.LN10)), f = x / p;
        return (f < 1.5 ? 1 : f < 3 ? 2 : f < 7 ? 5 : 10) * p;
    }

    function load(v) {
        var m = meta[v.key], ls = [], others = 0, min = Infinity, max = -Infinity, n = 0;
        for (var i = 0; i < v.series.length; i++) {
            var s = v.series[i], has = false;
            n = Math.max(n, s.values.length);
            for (var j = 0; j < s.values.length; j++) {
                var y = s.values[j];
                if (y === null)
                    continue;
                has = true;
                min = Math.min(min, s.lo ? s.lo[j] : y);
                max = Math.max(max, s.hi ? s.hi[j] : y);
            }
            if (has)
                ls.push({ name: s.name, here: s.here, outside: s.outside, values: s.values, lo: s.lo, hi: s.hi,
                          color: s.here ? hereColor : s.outside ? outsideColor : palette[others++ % palette.length] });
        }
        if (min === Infinity) {
            min = 0;
            max = 1;
        }
        var mid = (min + max) / 2, half = Math.max((max - min) / 2 * 1.15, m.span / 2);
        var a = mid - half, b = mid + half;
        if (min >= 0 && a < 0) {
            b -= a;
            a = 0;
        }
        var step = niceStep((b - a) / 4);
        lo = Math.floor(a / step) * step;
        hi = Math.ceil(b / step) * step;
        var t = [];
        for (var k = lo; k <= hi + step / 2; k += step)
            t.push(k);
        count = n;
        ticks = t;
        xticks = v.ticks || [];
        lines = ls;
        hist = v;
        canvas.requestPaint();
    }

    function px(i) { return count > 1 ? i / (count - 1) * plotW : 0; }
    function py(v) { return (hi - v) / (hi - lo) * plotH; }

    function fmt(v) {
        if (v === null || v === undefined)
            return "—";
        var m = meta[key];
        return v.toFixed(m.digits) + m.suffix;
    }

    function lastIndex(values) {
        for (var i = values.length - 1; i >= 0; i--)
            if (values[i] !== null)
                return i;
        return -1;
    }

    function shown(l) {
        var i = cursor >= 0 ? cursor : lastIndex(l.values);
        return i >= 0 ? l.values[i] : null;
    }

    // hist.start is local time counted as if it were UTC, so the UTC getters read local time.
    function timeAt(i) {
        if (!hist)
            return "";
        var d = new Date((hist.start + i * hist.step) * 1000), step = hist.step;
        function two(n) { return (n < 10 ? "0" : "") + n; }
        var hm = two(d.getUTCHours()) + ":" + two(d.getUTCMinutes());
        var date = monthNames[d.getUTCMonth()] + " " + d.getUTCDate();
        if (step <= 600)
            return hm;
        if (step <= 3600)
            return dayNames[d.getUTCDay()] + " " + hm;
        if (step < 86400)
            return date + " " + hm;
        var year = d.getUTCFullYear() !== new Date().getFullYear() ? ", " + d.getUTCFullYear() : "";
        return (step > 86400 ? "Week of " : "") + date + year;
    }

    function scrub(x) {
        idle.restart();
        hold.stop();
        if (count > 1)
            cursor = Math.max(0, Math.min(count - 1, Math.round((x - plotX) / plotW * (count - 1))));
    }

    onKeyChanged: {
        cursor = -1;
        if (visible)
            fetch();
    }
    onRangeChanged: {
        cursor = -1;
        if (visible)
            fetch();
    }
    onVisibleChanged: {
        cursor = -1;
        if (!visible) {
            range = "3h";
        } else {
            idle.restart();
            fetch();
        }
    }

    Timer { interval: 60000; repeat: true; running: view.visible; onTriggered: fetch() }
    Timer { id: idle; interval: 120000; running: view.visible; onTriggered: view.close() }
    Timer { id: hold; interval: 3000; onTriggered: cursor = -1 }

    MouseArea {
        anchors.fill: parent
        onPressed: idle.restart()
    }

    Row {
        x: 24; y: 20
        spacing: 10

        Repeater {
            model: tabs
            Rectangle {
                readonly property bool sel: modelData.key === key
                width: label.implicitWidth + 44
                height: 56
                radius: 28
                color: sel ? "#e6ebf1" : "#1a1f27"
                scale: tabArea.pressed ? 0.94 : 1
                Behavior on scale { NumberAnimation { duration: 80 } }

                Text {
                    id: label
                    anchors.centerIn: parent
                    text: modelData.label
                    textFormat: Text.RichText
                    font.family: fontMedium
                    font.pixelSize: 24
                    color: sel ? "#0b0d12" : "#b8c0cb"
                }
                MouseArea {
                    id: tabArea
                    anchors.fill: parent
                    anchors.margins: -6
                    onClicked: {
                        idle.restart();
                        view.pick(modelData.key);
                    }
                }
            }
        }
    }

    Item {
        x: 648; y: 24
        width: 48; height: 48

        Icon {
            anchors.centerIn: parent
            width: 30; height: 30
            kind: "close"
            color: "#9aa3b2"
        }
        MouseArea {
            anchors.fill: parent
            anchors.margins: -12
            onClicked: view.close()
        }
    }

    Text {
        id: value
        x: 24; y: 84
        text: current
        font.family: fontDin
        font.pixelSize: 104
        color: currentColor
    }
    Text {
        anchors.left: value.right
        anchors.leftMargin: 10
        anchors.baseline: value.baseline
        text: meta[key] ? meta[key].unit : ""
        font.family: fontFamily
        font.pixelSize: 28
        color: "#7d8796"
    }
    Row {
        anchors.right: parent.right
        anchors.rightMargin: 24
        y: 112
        spacing: 6

        Repeater {
            model: ranges
            Rectangle {
                readonly property bool sel: modelData.key === range
                width: 52
                height: 44
                radius: 22
                color: sel ? "#e6ebf1" : "#1a1f27"
                scale: rangeArea.pressed ? 0.94 : 1
                Behavior on scale { NumberAnimation { duration: 80 } }

                Text {
                    anchors.centerIn: parent
                    text: modelData.label
                    font.family: fontMedium
                    font.pixelSize: 20
                    color: sel ? "#0b0d12" : "#b8c0cb"
                }
                MouseArea {
                    id: rangeArea
                    anchors.fill: parent
                    anchors.margins: -4
                    onClicked: {
                        idle.restart();
                        range = modelData.key;
                    }
                }
            }
        }
    }

    Repeater {
        model: ready ? ticks : []
        Text {
            x: 0; width: plotX - 14
            y: plotY + py(modelData) - height / 2
            horizontalAlignment: Text.AlignRight
            text: modelData.toFixed(meta[key].digits === 0 || modelData % 1 === 0 ? 0 : 1)
            font.family: fontFamily
            font.pixelSize: 20
            color: "#5b6470"
        }
    }

    Repeater {
        model: ready ? xticks : []
        Text {
            x: plotX + px(modelData.i) - width / 2
            y: plotY + plotH + 12
            visible: px(modelData.i) > 30 && px(modelData.i) + width / 2 < plotW - nowLabel.width - 12
            text: modelData.label
            font.family: fontFamily
            font.pixelSize: 20
            color: "#5b6470"
        }
    }
    Text {
        id: nowLabel
        x: plotX + plotW - width
        y: plotY + plotH + 12
        visible: ready
        text: "Now"
        font.family: fontFamily
        font.pixelSize: 20
        color: "#5b6470"
    }

    Canvas {
        id: canvas
        readonly property real m: 10
        x: plotX - m; y: plotY - m
        width: plotW + 2 * m; height: plotH + 2 * m
        visible: ready

        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            if (!ready)
                return;
            var m = canvas.m, meta = view.meta[key], i, j;
            function X(i) { return m + px(i); }
            function Y(v) { return m + py(v); }

            for (i = 0; i < meta.bands.length; i++) {
                var b = meta.bands[i];
                var top = Y(Math.min(b[1], hi)), bottom = Y(Math.max(b[0], lo));
                if (bottom <= top)
                    continue;
                ctx.globalAlpha = 0.09;
                ctx.fillStyle = b[2];
                ctx.fillRect(m, top, plotW, bottom - top);
            }
            ctx.globalAlpha = 1;
            ctx.lineWidth = 1;
            ctx.strokeStyle = "#20262f";
            ctx.beginPath();
            for (i = 0; i < ticks.length; i++) {
                ctx.moveTo(m, Math.round(Y(ticks[i])) + 0.5);
                ctx.lineTo(m + plotW, Math.round(Y(ticks[i])) + 0.5);
            }
            for (i = 0; i < xticks.length; i++) {
                ctx.moveTo(Math.round(X(xticks[i].i)) + 0.5, m);
                ctx.lineTo(Math.round(X(xticks[i].i)) + 0.5, m + plotH);
            }
            ctx.stroke();

            ctx.lineCap = "round";
            ctx.lineJoin = "round";

            function trace(values, dashed) {
                var on = false, walked = 0, x0 = 0, y0 = 0;
                ctx.beginPath();
                for (var i = 0; i < values.length; i++) {
                    if (values[i] === null) {
                        on = false;
                        continue;
                    }
                    var x = X(i), y = Y(values[i]);
                    if (!on) {
                        ctx.moveTo(x, y);
                        on = true;
                    } else if (!dashed) {
                        ctx.lineTo(x, y);
                    } else {
                        var dx = x - x0, dy = y - y0, len = Math.sqrt(dx * dx + dy * dy), pos = 0;
                        while (pos < len) {
                            var phase = walked % 18, draw = phase < 9;
                            var seg = Math.min((draw ? 9 : 18) - phase, len - pos);
                            if (draw) {
                                ctx.moveTo(x0 + dx * pos / len, y0 + dy * pos / len);
                                ctx.lineTo(x0 + dx * (pos + seg) / len, y0 + dy * (pos + seg) / len);
                            }
                            pos += seg;
                            walked += seg;
                        }
                    }
                    x0 = x;
                    y0 = y;
                }
                ctx.stroke();
            }

            for (i = 0; i < lines.length; i++) {
                var l = lines[i];
                if (l.here)
                    continue;
                ctx.strokeStyle = l.color;
                ctx.lineWidth = 2.5;
                ctx.globalAlpha = 0.85;
                trace(l.values, l.outside);
                j = lastIndex(l.values);
                ctx.globalAlpha = 1;
                ctx.fillStyle = l.color;
                ctx.beginPath();
                ctx.arc(X(j), Y(l.values[j]), 4.5, 0, 2 * Math.PI);
                ctx.fill();
            }
            function band(lo, hi) {
                var i = 0;
                while (i < lo.length) {
                    if (lo[i] === null) {
                        i++;
                        continue;
                    }
                    var j = i;
                    while (j + 1 < lo.length && lo[j + 1] !== null)
                        j++;
                    if (j > i) {
                        ctx.beginPath();
                        ctx.moveTo(X(i), Y(hi[i]));
                        for (var k = i + 1; k <= j; k++)
                            ctx.lineTo(X(k), Y(hi[k]));
                        for (k = j; k >= i; k--)
                            ctx.lineTo(X(k), Y(lo[k]));
                        ctx.closePath();
                        ctx.fill();
                    }
                    i = j + 1;
                }
            }

            for (i = 0; i < lines.length; i++) {
                l = lines[i];
                if (!l.here || !l.lo)
                    continue;
                ctx.globalAlpha = 0.16;
                ctx.fillStyle = l.color;
                band(l.lo, l.hi);
                ctx.globalAlpha = 1;
            }
            for (i = 0; i < lines.length; i++) {
                l = lines[i];
                if (!l.here)
                    continue;
                ctx.strokeStyle = "#0b0d12";
                ctx.lineWidth = 10;
                trace(l.values, false);
                ctx.strokeStyle = l.color;
                ctx.lineWidth = 4.5;
                trace(l.values, false);
                j = lastIndex(l.values);
                ctx.fillStyle = "#0b0d12";
                ctx.beginPath();
                ctx.arc(X(j), Y(l.values[j]), 10, 0, 2 * Math.PI);
                ctx.fill();
                ctx.fillStyle = l.color;
                ctx.beginPath();
                ctx.arc(X(j), Y(l.values[j]), 7, 0, 2 * Math.PI);
                ctx.fill();
            }
        }
    }

    Text {
        anchors.centerIn: canvas
        visible: !ready || lines.length === 0
        text: ready ? "No data" : "Loading…"
        font.family: fontFamily
        font.pixelSize: 24
        color: "#5b6470"
    }

    Item {
        visible: cursor >= 0 && ready
        x: plotX + px(Math.max(cursor, 0))
        y: plotY

        Rectangle {
            x: -1
            width: 2; height: plotH
            color: "#e6ebf1"
            opacity: 0.45
        }
        Repeater {
            model: lines
            Rectangle {
                readonly property var v: cursor >= 0 ? modelData.values[cursor] : null
                visible: v !== null
                x: -width / 2
                y: v === null ? 0 : py(v) - height / 2
                width: modelData.here ? 16 : 12
                height: width
                radius: width / 2
                color: modelData.color
                border.width: 3
                border.color: "#0b0d12"
            }
        }
        Rectangle {
            x: Math.max(-plotX + 8, Math.min(720 - plotX - 8 - width, -width / 2))
            y: -height - 8
            width: when.implicitWidth + 24
            height: 34
            radius: 17
            color: "#e6ebf1"

            Text {
                id: when
                anchors.centerIn: parent
                text: timeAt(Math.max(cursor, 0))
                font.family: fontMedium
                font.pixelSize: 20
                color: "#0b0d12"
            }
        }
    }

    MouseArea {
        x: plotX - 20; y: plotY - 20
        width: plotW + 40; height: plotH + 40
        enabled: ready && lines.length > 0
        preventStealing: true
        onPressed: scrub(mouse.x + x)
        onPositionChanged: scrub(mouse.x + x)
        onReleased: hold.restart()
        onCanceled: hold.restart()
    }

    Flow {
        x: 24; y: 584
        width: 672
        spacing: 8

        Repeater {
            model: lines
            Row {
                id: entry
                readonly property var line: modelData
                spacing: 10
                rightPadding: 20
                height: 44

                Item {
                    width: 26; height: parent.height
                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 4
                        Repeater {
                            model: modelData.outside ? 3 : 1
                            Rectangle {
                                width: entry.line.outside ? 6 : 26
                                height: entry.line.here ? 6 : 4
                                radius: height / 2
                                color: entry.line.color
                            }
                        }
                    }
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.name
                    font.family: modelData.here ? fontMedium : fontFamily
                    font.pixelSize: 22
                    color: modelData.here ? "#e6ebf1" : "#9aa3b2"
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: modelData.here && cursor < 0 ? current : fmt(shown(modelData))
                    font.family: fontDin
                    font.pixelSize: 36
                    color: modelData.here ? "#f4f6f9" : modelData.color
                }
            }
        }
    }
}
