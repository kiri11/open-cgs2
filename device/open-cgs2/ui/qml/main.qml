import QtQuick 2.14
import QtQuick.Window 2.14

// The screen: the home page in the middle, component pages to its left, right, top and bottom,
// and component widgets on the home page and above everything. `app` (this window) is what the
// components see; see docs/components.md.
Window {
    id: root
    visible: true
    width: 720
    height: 720
    color: "#0d1015"

    readonly property string api: "http://127.0.0.1:8099"
    property var st: ({ ha: false, sensors: {}, groups: [], components: [], assist: { phase: "idle" } })
    property int version: 0
    property bool backend: false
    // The open full-screen view of a component, such as "spotify".
    property string modal: ""
    property bool chartOpen: false
    property bool airOpen: false
    property bool settingsOpen: false
    property bool clockOpen: false
    property double nowMs: Date.now()
    property bool lightAdjusting: false
    property bool talkHeld: false
    property bool replyHeld: false
    property string chartKey: "co2"
    // Loaded component pages by component name.
    property var pageItems: ({})
    // The device's fonts; elsewhere (the emulator) the same families when they are installed.
    readonly property string fontDin: din.status === FontLoader.Ready ? din.name : "DIN Condensed"
    readonly property string fontFamily: roboto.status === FontLoader.Ready ? roboto.name : "Roboto"
    readonly property string fontMedium: robotoMedium.status === FontLoader.Ready ? robotoMedium.name : "Roboto Medium"
    readonly property var components: st.components || []
    readonly property var intercom: st.intercom || null
    readonly property bool talking: talkHeld || replyHeld || (intercom !== null && intercom.talking)
    readonly property string phase: st.assist.phase
    readonly property bool online: backend && st.ha
    readonly property bool screenOn: st.screen !== false
    readonly property bool fahrenheit: st.units !== undefined && st.units.temperature === "F"
    readonly property var pm: st.pm || ({})
    readonly property var levelNames: ["Good", "Fair", "Poor", "Bad"]
    readonly property var levelColors: ["#4cd07d", "#f2c94c", "#f2994a", "#eb5757"]
    // Upper bounds of Good, Fair and Poor: Sensirion's VOC index 150/250/400 after the stock
    // app's remapping, and the US EPA breakpoints for particulates (µg/m³).
    readonly property var limits: ({ tvoc: [66, 92, 137], pm25: [9, 35.4, 55.4], pm10: [54, 154, 254] })
    readonly property var airKeys: ["tvoc", "pm25", "pm10"]
    readonly property int airLevel: Math.max(level("tvoc"), level("pm25"), level("pm10"))
    readonly property var clockState: st.clock || ({ timers: [], alarms: [] })
    readonly property var ringing: clockState.ringing || null
    readonly property bool idle: !talking && !lightAdjusting && modal === "" && !chartOpen && !airOpen && !clockOpen && phase === "idle" && screenOn
    readonly property double timerLeft: {
        var best = -1;
        for (var i = 0; i < clockState.timers.length; i++) {
            var t = clockState.timers[i], left = t.running ? Math.max(0, t.end - nowMs) : t.left;
            if (best < 0 || left < best)
                best = left;
        }
        return best;
    }
    readonly property var nextAlarm: {
        var now = new Date(nowMs), cur = now.getHours() * 60 + now.getMinutes(), best = null;
        if (clockState.snoozed)
            best = { time: Qt.formatTime(new Date(clockState.snoozed), "HH:mm"), minutes: Math.ceil((clockState.snoozed - nowMs) / 60000) };
        for (var i = 0; i < clockState.alarms.length; i++) {
            var a = clockState.alarms[i];
            if (!a.on)
                continue;
            var at = parseInt(a.time.slice(0, 2)) * 60 + parseInt(a.time.slice(3));
            for (var d = 0; d < 8; d++) {
                var minutes = at - cur + d * 1440;
                if (minutes > 0 && (a.days.length === 0 || a.days.indexOf((now.getDay() + d) % 7) >= 0)) {
                    if (!best || minutes < best.minutes)
                        best = { time: a.time, minutes: minutes };
                    break;
                }
            }
        }
        return best;
    }

    // Pages by direction from home: `pages` in config.json, else each component's `position`.
    readonly property var placed: {
        var out = { left: [], right: [], up: [], down: [] }, pages = st.pages || {}, names = {};
        for (var i = 0; i < components.length; i++)
            if (components[i].page)
                names[components[i].name] = components[i];
        for (var dir in out) {
            var listed = pages[dir];
            if (listed)
                for (var j = 0; j < listed.length; j++)
                    if (names[listed[j]]) {
                        out[dir].push(names[listed[j]]);
                        delete names[listed[j]];
                    }
        }
        for (var n in names)
            if (out[names[n].position] && !pages[names[n].position])
                out[names[n].position].push(names[n]);
        return out;
    }
    readonly property var rowPages: placed.left.slice().reverse().concat([null], placed.right)
    readonly property var columnPages: placed.up.slice().reverse().concat([null], placed.down)
    readonly property int homeColumn: placed.left.length
    readonly property int homeRow: placed.up.length

    onScreenOnChanged: if (!screenOn) goHome()
    onStChanged: Fmt.clock24 = !st.units || st.units.clock !== "12h"
    // `cgs2 screenshot` asks for one through the panel: it opens the view, saves it and goes home.
    readonly property double shotAt: st.shot ? st.shot.at : 0
    onShotAtChanged: if (shotAt > Date.now() - 5000) { showView(st.shot.view); shotTimer.restart(); }
    Timer {
        id: shotTimer
        interval: 1500
        onTriggered: contentItem.grabToImage(function(r) {
            r.saveToFile("/tmp/open-cgs2-shot.png");
            if (st.shot.view) { modal = ""; airOpen = false; chartOpen = false; goHome(); }
        })
    }
    // The window's own color is not part of a grab.
    Rectangle { anchors.fill: parent; color: root.color }

    FontLoader { id: din; source: "file:///usr/share/fonts/DIN_Condensed_QP.ttf" }
    FontLoader { id: roboto; source: "file:///usr/share/fonts/Roboto-Regular.ttf" }
    FontLoader { id: robotoMedium; source: "file:///usr/share/fonts/Roboto-Medium.ttf" }

    function poll() {
        var x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE)
                return;
            if (x.status === 200) {
                try {
                    var s = JSON.parse(x.responseText);
                    version = s.v;
                    st = s;
                    backend = true;
                } catch (e) {}
                poll();
            } else {
                backend = false;
                retry.start();
            }
        };
        x.open("GET", api + "/state?v=" + version);
        x.send();
    }

    function post(path, body) {
        var x = new XMLHttpRequest();
        x.open("POST", api + path);
        x.send(body === undefined ? "" : JSON.stringify(body));
    }

    // GETs `path` on the panel and passes the parsed JSON (or null) to `done`.
    function get(path, done) {
        var x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE)
                return;
            var v = null;
            try { if (x.status === 200) v = JSON.parse(x.responseText); } catch (e) {}
            done(v);
        };
        x.open("GET", api + path);
        x.send();
    }

    // Fetches any URL through the panel, which has the CA certificates.
    function fetch(url, done) {
        get("/fetch?url=" + encodeURIComponent(url), done);
    }

    // Calls a Home Assistant service, such as callService("light", "toggle", { entity_id: "light.desk" }).
    function callService(domain, service, target, data) {
        post("/ha/call/" + domain + "/" + service, { target: target || null, data: data || {} });
    }

    function entity(id) {
        return st.entities && st.entities[id] ? st.entities[id] : { state: null, attributes: {} };
    }

    function sensor(key) {
        var raw = key === "pm25" || key === "pm10" ? (pm.state === "on" ? pm[key] : undefined) : st.sensors[key];
        var n = parseFloat(raw);
        return isNaN(n) ? null : n;
    }

    function level(key) {
        var v = sensor(key);
        if (v === null || !limits[key])
            return -1;
        for (var i = 0; i < 3; i++)
            if (v <= limits[key][i])
                return i;
        return 3;
    }

    function pmNote() {
        return pm.state === "warming" ? "Starting…" : pm.state === "fault" ? "Not responding" : pm.state === "off" || pm.state === "night" ? "Sensor off" : "No data";
    }

    function format(key) {
        var v = sensor(key);
        if (v === null)
            return "—";
        return key === "temp" ? v.toFixed(1) + "°" : key === "rh" ? Math.round(v) + "%" : Math.round(v);
    }

    function co2Color(v) {
        return v === null ? "#5b6470" : v < 1000 ? "#4cd07d" : v < 1400 ? "#f2c94c" : "#eb5757";
    }

    function countdown(ms) {
        var s = Math.ceil(ms / 1000), h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60);
        return (h > 0 ? h + ":" + (m < 10 ? "0" : "") + m : m) + ":" + (s % 60 < 10 ? "0" : "") + s % 60;
    }

    // "in 3 hours", "in 40 min" or "now" for a time in milliseconds.
    function untilText(ms) {
        var left = ms - nowMs, minutes = Math.round(left / 60000), hours = Math.round(minutes / 60);
        return left <= 0 ? "now" : minutes < 60 ? "in " + Math.max(1, minutes) + " min" : "in " + hours + (hours === 1 ? " hour" : " hours");
    }

    function goHome() {
        settingsOpen = false;
        clockOpen = false;
        pages.currentIndex = homeColumn;
        if (homePages)
            homePages.currentIndex = homeRow;
    }

    // A view by the name `cgs2 screenshot` and the emulator use: a component page, music, air,
    // chart, clock or settings.
    function showView(view) {
        if (view === "music") modal = "spotify";
        else if (view === "air") airOpen = true;
        else if (view === "chart") chartOpen = true;
        else if (view === "clock") clockOpen = true;
        else if (view === "settings") settingsOpen = true;
        else if (view) openPage(view, false);
    }

    function openClock() { clockOpen = true; }
    function pageTouched() { pageIdle.restart(); }

    function openChart(key) {
        chartKey = key;
        chartOpen = true;
    }

    function page(name) {
        return pageItems[name] || null;
    }

    // Swipes to a component's page and passes `arg` to its open(), if it has one.
    function openPage(name, arg) {
        var item = page(name);
        for (var i = 0; i < rowPages.length; i++)
            if (rowPages[i] && rowPages[i].name === name)
                pages.currentIndex = i;
        for (var j = 0; j < columnPages.length; j++)
            if (columnPages[j] && columnPages[j].name === name)
                homePages.currentIndex = j;
        if (item && item.open)
            item.open(arg);
    }

    function componentUrl(c, slot) {
        return Qt.resolvedUrl("../components/" + c.name + "/" + c[slot]);
    }

    function slot(name) {
        var out = [];
        for (var i = 0; i < components.length; i++)
            if (components[i][name])
                out.push(components[i]);
        out.sort(function(a, b) { return a.order - b.order; });
        return out;
    }

    function registerPage(name, item) {
        var p = pageItems;
        p[name] = item;
        pageItems = Object.assign({}, p);
    }

    Timer { id: retry; interval: 1000; onTriggered: poll() }
    Timer { interval: clockState.timers.length > 0 ? 250 : 1000; running: true; repeat: true; onTriggered: nowMs = Date.now() }
    Component.onCompleted: poll()

    property var homePages: null

    Component {
        id: pageLoader
        Loader {
            id: loader
            property var desc
            property bool current
            width: 720; height: 720
            source: desc ? root.componentUrl(desc, "page") : ""
            onLoaded: {
                item.app = root;
                item.config = desc.config;
                item.active = Qt.binding(function() { return loader.current && root.screenOn; });
                root.registerPage(desc.name, item);
            }
        }
    }

    ListView {
        id: pages
        anchors.fill: parent
        orientation: ListView.Horizontal
        snapMode: ListView.SnapOneItem
        highlightRangeMode: ListView.StrictlyEnforceRange
        highlightMoveDuration: 250
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: 720 * 8
        currentIndex: homeColumn
        onCountChanged: positionViewAtIndex(homeColumn, ListView.SnapPosition)
        interactive: idle && (homePages === null || homePages.currentIndex === homeRow)
        model: rowPages.length
        delegate: Loader {
            readonly property var desc: rowPages[index]
            width: 720; height: 720
            sourceComponent: desc === null ? homeColumnComponent : pageLoader
            onLoaded: if (desc !== null) { item.desc = desc; item.current = Qt.binding(function() { return pages.currentIndex === index; }); }
        }
    }

    Component {
        id: homeColumnComponent
        ListView {
            id: column
            width: 720; height: 720
            clip: true
            snapMode: ListView.SnapOneItem
            highlightRangeMode: ListView.StrictlyEnforceRange
            highlightMoveDuration: 250
            boundsBehavior: Flickable.StopAtBounds
            cacheBuffer: 720 * 8
            currentIndex: homeRow
            onCountChanged: positionViewAtIndex(homeRow, ListView.SnapPosition)
            Component.onCompleted: root.homePages = column
            interactive: idle && pages.currentIndex === homeColumn
            model: columnPages.length
            delegate: Loader {
                readonly property var desc: columnPages[index]
                width: 720; height: 720
                sourceComponent: desc === null ? homeComponent : pageLoader
                onLoaded: if (desc !== null) { item.desc = desc; item.current = Qt.binding(function() { return column.currentIndex === index && pages.currentIndex === homeColumn; }); }
            }
        }
    }

    Component {
        id: homeComponent
        Item {
            width: 720; height: 720

            Row {
                id: statusRow
                x: 32; y: 8
                spacing: 16

                Row {
                    id: battery
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 6
                    readonly property var level: sensor("battery")
                    visible: level !== null && level >= 0 && level < 100

                    Item {
                        width: 24; height: 22
                        Rectangle {
                            x: 0; y: 5; width: 20; height: 12
                            radius: 2
                            color: "transparent"
                            border.color: "#9aa3b2"
                            Rectangle {
                                x: 3; y: 3
                                width: 14 * Math.max(0, Math.min(100, battery.level || 0)) / 100
                                height: 6
                                color: "#9aa3b2"
                            }
                        }
                        Rectangle {
                            x: 21; y: 8; width: 2; height: 6
                            radius: 1
                            color: "#9aa3b2"
                        }
                    }
                    Text {
                        text: battery.level === null ? "" : Math.round(battery.level) + "%"
                        font.family: fontFamily
                        font.pixelSize: 18
                        color: "#9aa3b2"
                    }
                }
                Repeater {
                    model: slot("status")
                    Loader {
                        id: statusLoader
                        anchors.verticalCenter: parent.verticalCenter
                        source: componentUrl(modelData, "status")
                        onLoaded: {
                            item.app = root;
                            item.config = modelData.config;
                            item.room = Qt.binding(function() { return Math.max(0, buttons.x - 24 - statusRow.x - statusLoader.x); });
                        }
                    }
                }
            }

            Item {
                id: sensors
                x: 32; y: 24
                width: 420; height: 320

                Text {
                    id: co2
                    y: -14
                    text: sensor("co2") === null ? "—" : Math.round(sensor("co2"))
                    font.family: fontDin
                    font.pixelSize: 210
                    color: co2Color(sensor("co2"))
                    opacity: co2Area.pressed ? 0.6 : 1
                    Behavior on color { ColorAnimation { duration: 400 } }
                }
                Text {
                    id: ppm
                    anchors.left: co2.right
                    anchors.leftMargin: 12
                    anchors.baseline: co2.baseline
                    text: "ppm"
                    font.family: fontFamily
                    font.pixelSize: 30
                    color: "#7d8796"
                }
                MouseArea {
                    id: co2Area
                    width: ppm.x + ppm.width; height: 196
                    onClicked: openChart("co2")
                }

                Row {
                    y: 206
                    spacing: 40
                    Repeater {
                        model: [
                            { key: "air", label: "Air" },
                            { key: "temp", label: "Temp" },
                            { key: "rh", label: "Humidity" }
                        ]
                        Item {
                            width: reading.width; height: reading.height
                            opacity: readingArea.pressed ? 0.6 : 1

                            Column {
                                id: reading
                                spacing: -8
                                Text {
                                    readonly property bool air: modelData.key === "air"
                                    text: !air ? format(modelData.key) : airLevel < 0 ? "—" : levelNames[airLevel]
                                    font.family: fontDin
                                    font.pixelSize: 72
                                    color: air && airLevel >= 0 ? levelColors[airLevel] : "#e6ebf1"
                                }
                                Text {
                                    text: modelData.label
                                    font.family: fontFamily
                                    font.pixelSize: 22
                                    color: "#7d8796"
                                }
                            }
                            MouseArea {
                                id: readingArea
                                anchors.fill: parent
                                anchors.margins: -16
                                onClicked: {
                                    if (modelData.key === "air")
                                        airOpen = true;
                                    else
                                        openChart(modelData.key);
                                }
                            }
                        }
                    }
                }
            }

            Column {
                id: clock
                anchors.right: parent.right
                anchors.rightMargin: 32
                y: 14
                opacity: clockArea.pressed ? 0.6 : 1
                readonly property bool showAlarm: timerLeft < 0 && nextAlarm !== null && nextAlarm.minutes <= 1440

                Text {
                    id: clockTime
                    anchors.right: parent.right
                    text: Fmt.time(new Date(nowMs))
                    font.family: fontDin
                    font.pixelSize: 48
                    color: "#9aa3b2"
                }
                Row {
                    anchors.right: parent.right
                    spacing: 6
                    visible: timerLeft >= 0 || clock.showAlarm
                    Icon {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 22; height: 22
                        kind: timerLeft >= 0 ? "timer" : "bell"
                        color: timerLeft >= 0 ? "#f2c94c" : "#7d8796"
                    }
                    Text {
                        text: timerLeft >= 0 ? countdown(timerLeft) : clock.showAlarm ? Fmt.hm(nextAlarm.time) : ""
                        font.family: fontDin
                        font.pixelSize: 30
                        color: timerLeft >= 0 ? "#f2c94c" : "#7d8796"
                    }
                }
            }
            MouseArea {
                id: clockArea
                x: clock.x - 16; y: 0
                width: clock.width + 32; height: clock.y + clock.height + 16
                onClicked: clockOpen = true
            }

            Row {
                id: buttons
                x: clock.x + clock.width - clockTime.width - 18 - width
                y: clock.y + clockTime.height / 2 - height / 2
                spacing: 18
                Repeater {
                    model: slot("button")
                    Loader {
                        source: componentUrl(modelData, "button")
                        onLoaded: { item.app = root; item.config = modelData.config; }
                    }
                }
            }

            Column {
                id: side
                x: 480; y: sideItems.count > 1 ? 96 : 124
                width: 208
                spacing: 16
                Repeater {
                    id: sideItems
                    model: slot("side")
                    Loader {
                        anchors.horizontalCenter: parent.horizontalCenter
                        source: componentUrl(modelData, "side")
                        onLoaded: {
                            item.app = root;
                            item.config = modelData.config;
                            item.compact = Qt.binding(function() { return sideItems.count > 1; });
                        }
                    }
                }
            }

            Repeater {
                model: slot("tiles")
                Loader {
                    x: 32; y: 368
                    source: componentUrl(modelData, "tiles")
                    onLoaded: { item.app = root; item.config = modelData.config; }
                }
            }
        }
    }

    Timer {
        id: pageIdle
        interval: 1800000
        running: pages.currentIndex !== homeColumn || (homePages !== null && homePages.currentIndex !== homeRow) || settingsOpen || clockOpen
        onTriggered: goHome()
    }

    AirView {
        anchors.fill: parent
        opacity: airOpen ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 150 } }
        readings: [
            { key: "tvoc", label: "eTVOC", unit: "", value: sensor("tvoc"), level: level("tvoc"), note: "No data" },
            { key: "pm25", label: "PM2.5", unit: "µg/m³", value: sensor("pm25"), level: level("pm25"), note: pmNote() },
            { key: "pm10", label: "PM10", unit: "µg/m³", value: sensor("pm10"), level: level("pm10"), note: pmNote() }
        ]
        rating: airLevel
        levelNames: root.levelNames
        levelColors: root.levelColors
        pm: root.pm
        fontDin: root.fontDin
        fontFamily: root.fontFamily
        fontMedium: root.fontMedium
        onClose: airOpen = false
        onChart: openChart(key)
        onSend: post(path)
    }

    ChartView {
        id: chartView
        anchors.fill: parent
        opacity: chartOpen ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 150 } }
        key: chartKey
        api: root.api
        tabs: airKeys.indexOf(chartKey) >= 0 ? airTabs : climateTabs
        current: format(chartKey)
        currentColor: chartKey === "co2" ? co2Color(sensor("co2")) : level(chartKey) >= 0 ? levelColors[level(chartKey)] : "#e6ebf1"
        fontDin: root.fontDin
        fontFamily: root.fontFamily
        fontMedium: root.fontMedium
        onPick: chartKey = key
        onClose: chartOpen = false
    }

    Loader {
        anchors.fill: parent
        active: settingsOpen
        source: "SettingsView.qml"
        onLoaded: {
            item.api = api;
            item.fontDin = fontDin;
            item.fontFamily = fontFamily;
            item.fontMedium = fontMedium;
            item.pm = Qt.binding(function() { return root.pm; });
            item.components = Qt.binding(function() { return root.components; });
            item.close.connect(function() { settingsOpen = false; });
            item.send.connect(post);
        }
    }

    Loader {
        anchors.fill: parent
        active: clockOpen
        source: "ClockView.qml"
        onLoaded: {
            item.api = api;
            item.fontDin = fontDin;
            item.fontFamily = fontFamily;
            item.fontMedium = fontMedium;
            item.clock = Qt.binding(function() { return clockState; });
            item.nextAlarm = Qt.binding(function() { return nextAlarm; });
            item.nowMs = Qt.binding(function() { return nowMs; });
            item.close.connect(function() { clockOpen = false; });
            item.send.connect(post);
        }
    }

    // Overlays below the ringing alarm (layer under 50), then the alarm, then the rest.
    Repeater {
        model: slot("overlay").filter(function(c) { return c.layer < 50; })
        Loader {
            anchors.fill: parent
            source: componentUrl(modelData, "overlay")
            onLoaded: { item.app = root; item.config = modelData.config; }
        }
    }

    RingView {
        anchors.fill: parent
        visible: ringing !== null
        ringing: root.ringing
        fontDin: root.fontDin
        fontFamily: root.fontFamily
        fontMedium: root.fontMedium
        onSend: post(path)
    }

    Repeater {
        model: slot("overlay").filter(function(c) { return c.layer >= 50; })
        Loader {
            anchors.fill: parent
            source: componentUrl(modelData, "overlay")
            onLoaded: { item.app = root; item.config = modelData.config; }
        }
    }

    Rectangle {
        id: powerMenu
        anchors.fill: parent
        visible: st.power_menu === true && screenOn
        color: "#e6000000"

        Timer { interval: 30000; running: powerMenu.visible; onTriggered: post("/power/cancel") }
        MouseArea {
            anchors.fill: parent
            onClicked: post("/power/cancel")
        }
        Column {
            anchors.centerIn: parent
            spacing: 24

            Repeater {
                model: [
                    { action: "off", label: "Power off", color: "#eb5757" },
                    { action: "reboot", label: "Restart", color: "#e6ebf1" },
                    { action: "settings", label: "Settings", color: "#e6ebf1" }
                ]
                Rectangle {
                    width: 420; height: 112
                    radius: 28
                    color: powerArea.pressed ? "#242b36" : "#161b22"

                    Text {
                        anchors.centerIn: parent
                        text: modelData.label
                        font.family: fontMedium
                        font.pixelSize: 36
                        color: modelData.color
                    }
                    MouseArea {
                        id: powerArea
                        anchors.fill: parent
                        onClicked: {
                            if (modelData.action === "settings") {
                                settingsOpen = true;
                                post("/power/cancel");
                            } else {
                                post("/power/" + modelData.action);
                            }
                        }
                    }
                }
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        visible: st.screen === false
        onPressed: post("/screen/wake")
    }
}
