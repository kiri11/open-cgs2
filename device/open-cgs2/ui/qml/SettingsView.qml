import QtQuick 2.14

Rectangle {
    id: view
    property string api
    property var pm: ({})
    property var components: []
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal close()
    signal send(string path)

    property var info: ({})
    property var prefs: ({ auto: true, level: 500, idle: 5 })
    property bool prefsLoaded: false
    // main, wifi or password
    property string page: "main"
    property var networks: []
    property bool scanning: false
    property string chosen: ""
    property string wifiNote: ""
    property string connectingTo: ""
    // "", "calibrate" or "stock"
    property string confirming: ""
    property bool calibrating: false
    property var calibratedBefore: null
    property string calibrationNote: ""
    readonly property var wifi: info.wifi || ({})
    readonly property var calibration: info.calibration || ({})
    readonly property int outdoorPpm: 420
    readonly property var adjust: info.adjust || ({})
    readonly property var adjustRows: [
        { key: "temperature_offset", label: "Temperature", step: 0.1, digits: 1, unit: "°", signed: true },
        { key: "humidity_offset", label: "Humidity", step: 1, digits: 0, unit: " pts", signed: true },
        { key: "humidity_scale", label: "Humidity scale", step: 0.01, digits: 2, unit: "×", signed: false }
    ]

    function setAdjust(key, value) {
        var body = {};
        for (var k in adjust)
            body[k] = adjust[k];
        body[key] = Math.round(value * 100) / 100;
        request("POST", "/calibration", body, function(status) { if (status === 200) refresh(); });
    }

    color: "#0b0d12"

    function request(method, path, body, done) {
        var x = new XMLHttpRequest();
        x.onreadystatechange = function() {
            if (x.readyState !== XMLHttpRequest.DONE)
                return;
            var v = null;
            try { v = JSON.parse(x.responseText); } catch (e) {}
            if (done)
                done(x.status, v);
        };
        x.open(method, api + path);
        if (body) {
            x.setRequestHeader("Content-Type", "application/json");
            x.send(JSON.stringify(body));
        } else {
            x.send();
        }
    }

    function refresh() {
        request("GET", "/settings", null, function(status, v) {
            if (status !== 200 || !v)
                return;
            info = v;
            if (!prefsLoaded) {
                prefs = v.screen;
                prefsLoaded = true;
            }
            if (connectingTo !== "" && v.wifi.connected && v.wifi.ssid === connectingTo) {
                connectingTo = "";
                wifiNote = "";
            }
            if (calibrating && v.calibration.time !== calibratedBefore) {
                calibrating = false;
                var c = v.calibration;
                calibrationNote = c.error ? "Calibration failed: " + c.error : "Calibrated: corrected by " + (c.correction > 0 ? "+" : "") + c.correction + " ppm";
            }
        });
    }

    function scan() {
        scanning = true;
        request("GET", "/wifi/scan", null, function(status, v) {
            scanning = false;
            if (status === 200 && v)
                networks = v;
        });
    }

    function connect(ssid, password) {
        wifiNote = "";
        request("POST", "/wifi/connect", { ssid: ssid, password: password }, function(status, v) {
            if (status === 200) {
                connectingTo = ssid;
                wifiNote = "Connecting to " + ssid + "…";
                connectTimeout.restart();
                page = "main";
            } else {
                wifiNote = v && v.error ? v.error : "Could not save the network";
            }
        });
    }

    function savePrefs() {
        request("POST", "/screen/prefs", prefs, null);
    }

    function setPref(key, value) {
        var p = { auto: prefs.auto, level: prefs.level, idle: prefs.idle };
        p[key] = value;
        prefs = p;
    }

    function shiftTime(hhmm, minutes) {
        var parts = (hhmm || "00:00").split(":");
        var m = (parseInt(parts[0]) * 60 + parseInt(parts[1]) + minutes + 1440) % 1440;
        var h = Math.floor(m / 60), mm = m % 60;
        return (h < 10 ? "0" : "") + h + ":" + (mm < 10 ? "0" : "") + mm;
    }

    function bars(rssi) {
        return rssi === undefined || rssi === null ? 0 : rssi > -55 ? 4 : rssi > -65 ? 3 : rssi > -75 ? 2 : 1;
    }

    function duration(s) {
        var d = Math.floor(s / 86400), h = Math.floor(s % 86400 / 3600), m = Math.floor(s % 3600 / 60);
        return d > 0 ? d + " d " + h + " h" : h > 0 ? h + " h " + m + " min" : m + " min";
    }

    onVisibleChanged: {
        if (visible) {
            page = "main";
            confirming = "";
            prefsLoaded = false;
            refresh();
            idle.restart();
        }
    }
    Component.onCompleted: refresh()

    Timer { interval: 2000; repeat: true; running: view.visible && (page === "main" || calibrating); onTriggered: refresh() }
    Timer { id: idle; interval: 180000; running: view.visible; onTriggered: view.close() }
    Timer { id: prefsDelay; interval: 150; onTriggered: savePrefs() }
    Timer {
        id: connectTimeout
        interval: 40000
        onTriggered: if (connectingTo !== "") {
            wifiNote = "Could not join " + connectingTo + "; check the password";
            connectingTo = "";
        }
    }
    Timer { id: calibrationTimeout; interval: 20000; onTriggered: if (calibrating) { calibrating = false; calibrationNote = "No answer from the sensor service"; } }

    MouseArea {
        anchors.fill: parent
    }

    Text {
        x: page === "main" ? 24 : 84
        y: 26
        text: page === "main" ? "Settings" : page === "wifi" ? "Wi-Fi" : chosen
        width: 560
        elide: Text.ElideRight
        font.family: fontMedium
        font.pixelSize: 30
        color: "#9aa3b2"
    }
    Item {
        x: 24; y: 24
        width: 48; height: 48
        visible: page !== "main"
        Icon {
            anchors.centerIn: parent
            width: 34; height: 34
            kind: "back"
            color: "#9aa3b2"
        }
        MouseArea {
            anchors.fill: parent
            anchors.margins: -12
            onClicked: page = page === "password" ? "wifi" : "main"
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

    Flickable {
        id: mainPage
        x: 24; y: 96
        width: 672; height: 624
        visible: page === "main"
        contentHeight: content.height + 32
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: content
            width: 672
            spacing: 16

            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 112
                opacity: wifiArea.pressed ? 0.6 : 1
                Column {
                    x: 32
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 4
                    Text {
                        text: "Wi-Fi"
                        font.family: fontMedium
                        font.pixelSize: 28
                        color: "#e6ebf1"
                    }
                    Text {
                        width: 480
                        elide: Text.ElideRight
                        text: wifiNote !== "" ? wifiNote : wifi.connected ? wifi.ssid + "  ·  " + (wifi.ip || "no address") : "Not connected"
                        font.family: fontFamily
                        font.pixelSize: 22
                        color: wifiNote !== "" && connectingTo === "" ? "#eb5757" : "#7d8796"
                    }
                }
                SignalBars {
                    x: parent.width - width - 32
                    anchors.verticalCenter: parent.verticalCenter
                    height: 26
                    level: wifi.connected ? bars(wifi.rssi) : 0
                }
                MouseArea {
                    id: wifiArea
                    anchors.fill: parent
                    onClicked: {
                        page = "wifi";
                        scan();
                    }
                }
            }

            Text { x: 32; text: "Screen"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 380
                Text {
                    x: 32; y: 30
                    text: "Automatic brightness"
                    font.family: fontMedium
                    font.pixelSize: 28
                    color: "#e6ebf1"
                }
                Toggle {
                    x: parent.width - width - 32; y: 24
                    on: prefs.auto
                    onToggled: {
                        setPref("auto", !prefs.auto);
                        savePrefs();
                    }
                }
                Item {
                    id: slider
                    x: 32; y: 100
                    width: 608; height: 64
                    opacity: prefs.auto ? 0.35 : 1
                    readonly property real fraction: (prefs.level - 20) / 980
                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width; height: 10
                        radius: 5
                        color: "#2a313c"
                        Rectangle {
                            width: Math.max(10, slider.fraction * parent.width)
                            height: parent.height
                            radius: 5
                            color: "#f2c94c"
                        }
                    }
                    Rectangle {
                        x: slider.fraction * (slider.width - width)
                        anchors.verticalCenter: parent.verticalCenter
                        width: 44; height: 44
                        radius: 22
                        color: "#f4f6f9"
                    }
                    MouseArea {
                        anchors.fill: parent
                        anchors.margins: -12
                        enabled: !prefs.auto
                        preventStealing: true
                        function pick(x) {
                            var f = Math.max(0, Math.min(1, (x - 22) / (slider.width - 44)));
                            setPref("level", Math.round(20 + f * 980));
                            prefsDelay.restart();
                        }
                        onPressed: pick(mouse.x - 12)
                        onPositionChanged: pick(mouse.x - 12)
                    }
                }
                Text {
                    x: 32; y: 184
                    text: "Turn off on battery after"
                    font.family: fontMedium
                    font.pixelSize: 28
                    color: "#e6ebf1"
                }
                Row {
                    x: 32; y: 240
                    spacing: 12
                    Repeater {
                        model: [{ m: 1, l: "1 min" }, { m: 5, l: "5 min" }, { m: 10, l: "10 min" }, { m: 30, l: "30 min" }, { m: 0, l: "Never" }]
                        Rectangle {
                            width: 112; height: 104
                            radius: 20
                            readonly property bool picked: prefs.idle === modelData.m
                            color: picked ? "#2d5b40" : idleArea.pressed ? "#2f3743" : "#232a34"
                            border.width: picked ? 2 : 0
                            border.color: "#4cd07d"
                            Text {
                                anchors.centerIn: parent
                                text: modelData.l
                                font.family: fontMedium
                                font.pixelSize: 24
                                color: "#e6ebf1"
                            }
                            MouseArea {
                                id: idleArea
                                anchors.fill: parent
                                onClicked: {
                                    setPref("idle", modelData.m);
                                    savePrefs();
                                }
                            }
                        }
                    }
                }
            }

            Text { x: 32; text: "Particulate sensor off at night"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 216
                opacity: pm.from === undefined ? 0.45 : 1
                Repeater {
                    model: [{ key: "from", label: "From" }, { key: "until", label: "Until" }]
                    Item {
                        x: 32; y: 20 + index * 96
                        width: 608; height: 80
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label
                            font.family: fontMedium
                            font.pixelSize: 28
                            color: "#e6ebf1"
                        }
                        Text {
                            x: 220
                            anchors.verticalCenter: parent.verticalCenter
                            text: pm[modelData.key] || "—"
                            font.family: fontDin
                            font.pixelSize: 56
                            color: pm.night === false ? "#7d8796" : "#e6ebf1"
                        }
                        TimeStepper {
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            enabled: pm.from !== undefined
                            onStep: {
                                var from = modelData.key === "from" ? shiftTime(pm.from, minutes) : pm.from;
                                var until = modelData.key === "until" ? shiftTime(pm.until, minutes) : pm.until;
                                view.send("/pm/window/" + from + "-" + until);
                            }
                        }
                    }
                }
            }

            Text { x: 32; text: "CO2 sensor"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 236
                Column {
                    x: 32; y: 24
                    width: 608
                    spacing: 6
                    Text {
                        text: "Now " + (info.co2 === undefined || info.co2 === null ? "—" : info.co2 + " ppm")
                        font.family: fontMedium
                        font.pixelSize: 28
                        color: "#e6ebf1"
                    }
                    Text {
                        width: 608
                        wrapMode: Text.WordWrap
                        text: calibrating ? "Calibrating…" : calibrationNote !== "" ? calibrationNote : calibration.time ? "Last calibrated " + new Date(calibration.time * 1000).toLocaleString(Qt.locale("en_US"), "d MMM yyyy, HH:mm") + (calibration.error ? " (failed)" : ", corrected by " + (calibration.correction > 0 ? "+" : "") + calibration.correction + " ppm") : "Calibrates itself against the lowest levels it sees each week"
                        font.family: fontFamily
                        font.pixelSize: 22
                        color: calibrationNote.indexOf("failed") >= 0 || calibrationNote.indexOf("No answer") >= 0 ? "#eb5757" : "#7d8796"
                    }
                }
                SettingsButton {
                    fontFamily: fontMedium
                    x: 32; y: 124
                    width: 608
                    label: "Calibrate to outdoor air"
                    enabled: !calibrating
                    onClicked: confirming = "calibrate"
                }
            }

            Text { x: 32; text: "Calibration"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 40 + adjustRows.length * 96
                Repeater {
                    model: adjustRows
                    Item {
                        x: 32; y: 20 + index * 96
                        width: 608; height: 80
                        readonly property var value: adjust[modelData.key]
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label
                            font.family: fontMedium
                            font.pixelSize: 28
                            color: "#e6ebf1"
                        }
                        Text {
                            x: 260
                            anchors.verticalCenter: parent.verticalCenter
                            text: parent.value === undefined ? "—" : (modelData.signed && parent.value > 0 ? "+" : "") + parent.value.toFixed(modelData.digits) + modelData.unit
                            font.family: fontDin
                            font.pixelSize: 48
                            color: "#e6ebf1"
                        }
                        TimeStepper {
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            enabled: parent.value !== undefined
                            onStep: setAdjust(modelData.key, parent.value + (minutes > 0 ? modelData.step : -modelData.step))
                        }
                    }
                }
            }

            Text { x: 32; text: "Components"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: componentText.height + 40
                Text {
                    id: componentText
                    x: 32; y: 20
                    width: 608
                    wrapMode: Text.WordWrap
                    text: components.length ? components.map(function(c) { return c.name; }).join(", ") : "None"
                    font.family: fontFamily
                    font.pixelSize: 24
                    color: "#e6ebf1"
                }
            }

            Text { x: 32; text: "Device"; font.family: fontMedium; font.pixelSize: 22; color: "#7d8796" }
            Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: deviceRows.height + 40
                Column {
                    id: deviceRows
                    x: 32; y: 20
                    width: 608
                    Repeater {
                        model: [
                            { l: "Name", v: info.name || "—" },
                            { l: "IP address", v: wifi.ip || "—" },
                            { l: "MAC address", v: (info.mac || "—").toUpperCase() },
                            { l: "Battery", v: info.battery === undefined || info.battery === null ? "—" : info.battery + "%" + (info.charging ? ", charging" : "") },
                            { l: "Up for", v: info.uptime ? duration(info.uptime) : "—" },
                            { l: "open-cgs2", v: info.version || "—" },
                            { l: "Firmware", v: info.firmware || "—" }
                        ]
                        Item {
                            width: 608; height: 52
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: modelData.l
                                font.family: fontFamily
                                font.pixelSize: 24
                                color: "#7d8796"
                            }
                            Text {
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                text: modelData.v
                                font.family: fontFamily
                                font.pixelSize: 24
                                color: "#e6ebf1"
                            }
                        }
                    }
                }
            }

            SettingsButton {
                fontFamily: fontMedium
                width: 672
                label: "Restart into stock firmware"
                textColor: "#f2994a"
                onClicked: confirming = "stock"
            }
        }
    }

    Item {
        id: wifiPage
        x: 24; y: 96
        width: 672; height: 624
        visible: page === "wifi"

        Text {
            visible: scanning || networks.length === 0
            x: 8; y: 8
            text: scanning ? "Looking for networks…" : "No networks found"
            font.family: fontFamily
            font.pixelSize: 26
            color: "#7d8796"
        }
        ListView {
            anchors.fill: parent
            anchors.bottomMargin: 120
            visible: !scanning
            clip: true
            spacing: 12
            model: networks
            delegate: Rectangle {
                width: 672
                radius: 24
                color: "#161b22"
                height: 96
                opacity: netArea.pressed ? 0.6 : 1
                readonly property bool current: wifi.connected && wifi.ssid === modelData.ssid
                Column {
                    x: 32
                    anchors.verticalCenter: parent.verticalCenter
                    Text {
                        width: 470
                        elide: Text.ElideRight
                        text: modelData.ssid
                        font.family: fontMedium
                        font.pixelSize: 28
                        color: "#e6ebf1"
                    }
                    Text {
                        visible: current
                        text: "Connected"
                        font.family: fontFamily
                        font.pixelSize: 20
                        color: "#4cd07d"
                    }
                }
                Icon {
                    x: parent.width - 120
                    anchors.verticalCenter: parent.verticalCenter
                    width: 26; height: 26
                    visible: modelData.secured
                    kind: "lock"
                    color: "#7d8796"
                }
                SignalBars {
                    x: parent.width - width - 32
                    anchors.verticalCenter: parent.verticalCenter
                    height: 26
                    level: bars(modelData.rssi)
                }
                MouseArea {
                    id: netArea
                    anchors.fill: parent
                    onClicked: {
                        chosen = modelData.ssid;
                        if (modelData.secured) {
                            password.text = "";
                            wifiNote = "";
                            page = "password";
                            password.forceActiveFocus();
                        } else {
                            connect(modelData.ssid, "");
                        }
                    }
                }
            }
        }
        SettingsButton {
            fontFamily: fontMedium
            x: 0; y: 520
            width: 672
            label: "Scan again"
            enabled: !scanning
            onClicked: scan()
        }
    }

    Item {
        id: passwordPage
        x: 24; y: 96
        width: 672; height: 624
        visible: page === "password"

        Rectangle {
            width: 672; height: 88
            radius: 20
            color: "#161b22"
            border.width: 2
            border.color: password.activeFocus ? "#4cd07d" : "#2a313c"
            TextInput {
                id: password
                x: 24
                width: 480
                anchors.verticalCenter: parent.verticalCenter
                echoMode: showPassword.on ? TextInput.Normal : TextInput.Password
                font.family: fontFamily
                font.pixelSize: 30
                color: "#e6ebf1"
                clip: true
                onAccepted: connect(chosen, text)
                Text {
                    visible: password.text === ""
                    text: "Password"
                    font: password.font
                    color: "#5b6470"
                }
            }
            Text {
                id: showPassword
                property bool on: false
                anchors.right: parent.right
                anchors.rightMargin: 24
                anchors.verticalCenter: parent.verticalCenter
                text: on ? "Hide" : "Show"
                font.family: fontMedium
                font.pixelSize: 24
                color: "#9aa3b2"
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -16
                    onClicked: showPassword.on = !showPassword.on
                }
            }
        }
        Text {
            y: 104
            width: 440
            wrapMode: Text.WordWrap
            text: wifiNote
            font.family: fontFamily
            font.pixelSize: 22
            color: "#eb5757"
        }
        SettingsButton {
            fontFamily: fontMedium
            x: 472; y: 100
            width: 200
            label: "Join"
            enabled: password.text.length >= 8
            onClicked: connect(chosen, password.text)
        }
    }

    MouseArea {
        anchors.fill: parent
        onPressed: {
            idle.restart();
            mouse.accepted = false;
        }
    }

    Rectangle {
        anchors.fill: parent
        visible: confirming !== ""
        color: "#0b0d12"
        MouseArea { anchors.fill: parent }
        Column {
            x: 48
            anchors.verticalCenter: parent.verticalCenter
            width: 624
            spacing: 24
            Text {
                text: confirming === "calibrate" ? "Calibrate the CO2 sensor" : "Restart into stock firmware"
                font.family: fontMedium
                font.pixelSize: 34
                color: "#e6ebf1"
            }
            Text {
                width: 624
                wrapMode: Text.WordWrap
                lineHeight: 1.15
                text: confirming === "calibrate"
                    ? "Put the monitor outdoors or next to a wide-open window and leave it there for at least 5 minutes, away from people breathing on it. Then press Calibrate: the sensor takes the air around it as " + outdoorPpm + " ppm.\n\nIt reads " + (info.co2 === undefined || info.co2 === null ? "—" : info.co2 + " ppm") + " now."
                    : "The Qingping app runs until the next restart. Restarting again (power key, hold for 1.5 s) comes back to this screen."
                font.family: fontFamily
                font.pixelSize: 26
                color: "#9aa3b2"
            }
            Row {
                spacing: 24
                SettingsButton {
                    fontFamily: fontMedium
                    width: 300
                    label: "Cancel"
                    onClicked: confirming = ""
                }
                SettingsButton {
                    fontFamily: fontMedium
                    width: 300
                    label: confirming === "calibrate" ? "Calibrate" : "Restart"
                    textColor: confirming === "calibrate" ? "#4cd07d" : "#f2994a"
                    onClicked: {
                        if (confirming === "calibrate") {
                            calibratedBefore = calibration.time;
                            calibrationNote = "";
                            calibrating = true;
                            calibrationTimeout.restart();
                            request("POST", "/co2/calibrate/" + outdoorPpm, null, function(status) {
                                if (status !== 200) {
                                    calibrating = false;
                                    calibrationNote = "Calibration failed: the request was refused";
                                }
                            });
                        } else {
                            request("POST", "/stock", null, null);
                        }
                        confirming = "";
                    }
                }
            }
        }
    }

    Keyboard {
        y: parent.height - height
        visible: page === "password" && confirming === ""
        fontFamily: view.fontMedium
        onTyped: password.insert(password.cursorPosition, text)
        onErase: if (password.cursorPosition > 0) password.remove(password.cursorPosition - 1, password.cursorPosition)
        onDone: if (password.text.length >= 8) connect(chosen, password.text)
    }
}
