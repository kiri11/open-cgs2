import QtQuick 2.14

Rectangle {
    id: view
    property string api
    property var clock: ({})
    property var nextAlarm: null
    property double nowMs: Date.now()
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal close()
    signal send(string path)

    // main, keypad, alarm or sound
    property string page: "main"
    property string tab: "timers"
    property string digits: ""
    property var tones: []
    property int editId: 0
    property var editDays: []
    property string editTone: "6e70b659"
    readonly property var timers: clock.timers || []
    readonly property var alarms: clock.alarms || []
    readonly property var presets: [60, 180, 300, 600, 900, 1200, 1800, 3600, 0]
    readonly property var dayOrder: [1, 2, 3, 4, 5, 6, 0]
    readonly property var dayNames: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    readonly property string padded: ("000000" + digits).slice(-6)
    readonly property int keypadSeconds: parseInt(padded.slice(0, 2)) * 3600 + parseInt(padded.slice(2, 4)) * 60 + parseInt(padded.slice(4))

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

    function two(n) {
        return (n < 10 ? "0" : "") + n;
    }

    function remaining(t) {
        return t.running ? Math.max(0, t.end - nowMs) : t.left;
    }

    function countdown(ms) {
        var s = Math.ceil(ms / 1000), h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60);
        return (h > 0 ? h + ":" + two(m) : m) + ":" + two(s % 60);
    }

    function span(s) {
        var parts = [];
        if (s >= 3600) parts.push(Math.floor(s / 3600) + " h");
        if (s % 3600 >= 60) parts.push(Math.floor(s % 3600 / 60) + " min");
        if (s % 60) parts.push(s % 60 + " s");
        return parts.join(" ");
    }

    function daysText(days) {
        if (!days || days.length === 0) return "Once";
        if (days.length === 7) return "Every day";
        if (days.join() === "1,2,3,4,5") return "Weekdays";
        if (days.join() === "0,6") return "Weekends";
        return dayOrder.filter(function(d) { return days.indexOf(d) >= 0; }).map(function(d) { return dayNames[d]; }).join(", ");
    }

    function inText(minutes) {
        var h = Math.floor(minutes / 60), m = minutes % 60;
        return (h > 0 ? h + " h " : "") + (h > 0 && m === 0 ? "" : m + " min");
    }

    function toneName(key) {
        for (var i = 0; i < tones.length; i++)
            if (tones[i].key === key)
                return tones[i].name;
        return "";
    }

    function editAlarm(a) {
        var parts = (a ? a.time : "07:00").split(":");
        editId = a ? a.id : 0;
        editDays = a ? a.days.slice() : [];
        editTone = a ? a.tone : "6e70b659";
        hourWheel.currentIndex = parseInt(parts[0]);
        minuteWheel.currentIndex = parseInt(parts[1]);
        page = "alarm";
    }

    function toggleDay(d) {
        var days = editDays.slice(), i = days.indexOf(d);
        if (i >= 0)
            days.splice(i, 1);
        else
            days.push(d);
        days.sort();
        editDays = days;
    }

    function saveAlarm() {
        var body = { id: editId, time: two(hourWheel.currentIndex) + ":" + two(minuteWheel.currentIndex), days: editDays, tone: editTone };
        request("POST", "/clock/alarm", body, null);
        page = "main";
        tab = "alarms";
    }

    function back() {
        if (page === "sound") {
            send("/clock/preview/stop");
            page = "alarm";
        } else {
            page = "main";
        }
    }

    function leave() {
        if (page === "sound")
            send("/clock/preview/stop");
        view.close();
    }

    Component.onCompleted: request("GET", "/clock/tones", null, function(status, v) {
        if (status === 200 && v)
            tones = v;
    })

    Timer { id: idle; interval: 180000; running: view.visible && timers.length === 0; onTriggered: leave() }

    MouseArea {
        anchors.fill: parent
    }

    Text {
        x: 84; y: 26
        visible: page !== "main"
        text: page === "keypad" ? "New timer" : page === "sound" ? "Sound" : editId ? "Edit alarm" : "New alarm"
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
            onClicked: back()
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
            onClicked: leave()
        }
    }

    Item {
        anchors.fill: parent
        visible: page === "main"

        Text {
            x: 24; y: -6
            text: Fmt.time(new Date(nowMs))
            font.family: fontDin
            font.pixelSize: 132
            color: "#e6ebf1"
        }
        Text {
            x: 28; y: 128
            text: new Date(nowMs).toLocaleDateString(Qt.locale("en_US"), "dddd, d MMMM")
            font.family: fontFamily
            font.pixelSize: 26
            color: "#7d8796"
        }

        Row {
            x: 24; y: 184
            spacing: 24
            Repeater {
                model: [{ key: "timers", label: "Timers", icon: "timer" }, { key: "alarms", label: "Alarms", icon: "bell" }]
                Rectangle {
                    width: 324; height: 76
                    radius: 38
                    readonly property bool picked: tab === modelData.key
                    readonly property int count: modelData.key === "timers" ? timers.length : 0
                    color: picked ? "#2a313c" : tabArea.pressed ? "#1d232c" : "#161b22"
                    Row {
                        anchors.centerIn: parent
                        spacing: 12
                        Icon {
                            anchors.verticalCenter: parent.verticalCenter
                            width: 30; height: 30
                            kind: modelData.icon
                            color: picked ? "#e6ebf1" : "#7d8796"
                        }
                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label + (count > 0 ? "  " + count : "")
                            font.family: fontMedium
                            font.pixelSize: 28
                            color: picked ? "#e6ebf1" : "#7d8796"
                        }
                    }
                    MouseArea {
                        id: tabArea
                        anchors.fill: parent
                        onClicked: tab = modelData.key
                    }
                }
            }
        }

        Flickable {
            x: 24; y: 284
            width: 672; height: 436
            visible: tab === "timers"
            contentHeight: timerColumn.height + 24
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: timerColumn
                width: 672
                spacing: 16

                Repeater {
                    model: timers.length
                    Rectangle {
                        readonly property var t: timers[index] || ({ total: 1, running: false, left: 0 })
                        readonly property double ms: remaining(t)
                        width: 672; height: 128
                        radius: 24
                        color: "#161b22"
                        Text {
                            id: timeLeft
                            x: 32; y: 4
                            text: countdown(ms)
                            font.family: fontDin
                            font.pixelSize: 96
                            color: t.running ? "#e6ebf1" : "#7d8796"
                        }
                        Text {
                            anchors.left: timeLeft.right
                            anchors.leftMargin: 16
                            anchors.baseline: timeLeft.baseline
                            text: t.running ? span(t.total) : "Paused"
                            font.family: fontFamily
                            font.pixelSize: 22
                            color: "#7d8796"
                        }
                        Row {
                            anchors.right: parent.right
                            anchors.rightMargin: 24
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 16
                            Repeater {
                                model: [t.running ? "pause" : "play", "close"]
                                Rectangle {
                                    width: 80; height: 80
                                    radius: 40
                                    color: timerButton.pressed ? "#2f3743" : "#232a34"
                                    Icon {
                                        anchors.centerIn: parent
                                        width: 34; height: 34
                                        kind: modelData
                                        color: "#e6ebf1"
                                    }
                                    MouseArea {
                                        id: timerButton
                                        anchors.fill: parent
                                        onClicked: send("/clock/timer/" + t.id + "/" + (modelData === "close" ? "cancel" : t.running ? "pause" : "resume"))
                                    }
                                }
                            }
                        }
                        Rectangle {
                            x: 24; y: parent.height - 14
                            width: 624; height: 6
                            radius: 3
                            color: "#232a34"
                            Rectangle {
                                width: Math.max(6, parent.width * ms / (t.total * 1000))
                                height: parent.height
                                radius: 3
                                color: "#f2c94c"
                            }
                        }
                    }
                }

                Grid {
                    columns: 3
                    columnSpacing: 24
                    rowSpacing: 16
                    Repeater {
                        model: presets
                        Rectangle {
                            width: 208; height: 88
                            radius: 24
                            color: presetArea.pressed ? "#242b36" : "#161b22"
                            Text {
                                anchors.centerIn: parent
                                text: modelData ? span(modelData) : "Custom"
                                font.family: fontMedium
                                font.pixelSize: 28
                                color: modelData ? "#e6ebf1" : "#9aa3b2"
                            }
                            MouseArea {
                                id: presetArea
                                anchors.fill: parent
                                onClicked: {
                                    if (modelData) {
                                        send("/clock/timer/start/" + modelData);
                                    } else {
                                        digits = "";
                                        page = "keypad";
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Flickable {
            x: 24; y: 284
            width: 672; height: 436
            visible: tab === "alarms"
            contentHeight: alarmColumn.height + 24
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: alarmColumn
                width: 672
                spacing: 16

                Item {
                    width: 672; height: 36
                    visible: nextAlarm !== null || !!clock.snoozed
                    Text {
                        x: 8
                        anchors.verticalCenter: parent.verticalCenter
                        text: clock.snoozed ? "Snoozed until " + Fmt.time(new Date(clock.snoozed)) : nextAlarm ? "Next alarm in " + inText(nextAlarm.minutes) : ""
                        font.family: fontFamily
                        font.pixelSize: 24
                        color: "#9aa3b2"
                    }
                    Text {
                        anchors.right: parent.right
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        visible: !!clock.snoozed
                        text: "Cancel snooze"
                        font.family: fontMedium
                        font.pixelSize: 24
                        color: "#f2994a"
                        MouseArea {
                            anchors.fill: parent
                            anchors.margins: -16
                            onClicked: send("/clock/snooze/cancel")
                        }
                    }
                }

                Repeater {
                    model: alarms.length
                    Rectangle {
                        readonly property var a: alarms[index] || ({ id: 0, time: "", days: [], on: false, tone: "" })
                        width: 672; height: 120
                        radius: 24
                        color: alarmArea.pressed ? "#1d232c" : "#161b22"
                        MouseArea {
                            id: alarmArea
                            anchors.fill: parent
                            onClicked: editAlarm(a)
                        }
                        Text {
                            id: alarmTime
                            x: 32; y: 2
                            text: Fmt.hm(a.time)
                            font.family: fontDin
                            font.pixelSize: 96
                            color: a.on ? "#e6ebf1" : "#5b6470"
                        }
                        Text {
                            anchors.left: alarmTime.right
                            anchors.leftMargin: 20
                            anchors.baseline: alarmTime.baseline
                            width: 300
                            elide: Text.ElideRight
                            text: daysText(a.days)
                            font.family: fontFamily
                            font.pixelSize: 24
                            color: a.on ? "#9aa3b2" : "#5b6470"
                        }
                        Toggle {
                            anchors.right: parent.right
                            anchors.rightMargin: 32
                            anchors.verticalCenter: parent.verticalCenter
                            on: a.on
                            onToggled: send("/clock/alarm/" + a.id + (a.on ? "/off" : "/on"))
                        }
                    }
                }

                SettingsButton {
                    fontFamily: fontMedium
                    width: 672
                    label: "Add alarm"
                    onClicked: editAlarm(null)
                }
            }
        }
    }

    Item {
        x: 24; y: 96
        width: 672; height: 624
        visible: page === "keypad"

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 20
            Repeater {
                model: ["h", "m", "s"]
                Row {
                    spacing: 6
                    Text {
                        id: pair
                        text: padded.slice(index * 2, index * 2 + 2)
                        font.family: fontDin
                        font.pixelSize: 120
                        color: digits.length > 4 - index * 2 ? "#e6ebf1" : "#5b6470"
                    }
                    Text {
                        anchors.baseline: pair.baseline
                        text: modelData
                        font.family: fontMedium
                        font.pixelSize: 32
                        color: "#7d8796"
                    }
                }
            }
        }

        Grid {
            y: 140
            columns: 3
            columnSpacing: 16
            rowSpacing: 12
            Repeater {
                model: ["1", "2", "3", "4", "5", "6", "7", "8", "9", "00", "0", "erase"]
                Rectangle {
                    width: 213; height: 80
                    radius: 20
                    color: keyArea.pressed ? "#3a4350" : "#1d232c"
                    Text {
                        anchors.centerIn: parent
                        visible: modelData !== "erase"
                        text: modelData
                        font.family: fontMedium
                        font.pixelSize: 36
                        color: "#e6ebf1"
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: modelData === "erase"
                        width: 40; height: 40
                        kind: "erase"
                        color: "#e6ebf1"
                    }
                    MouseArea {
                        id: keyArea
                        anchors.fill: parent
                        onClicked: {
                            if (modelData === "erase")
                                digits = digits.slice(0, -1);
                            else if (digits !== "" || modelData.charAt(0) !== "0")
                                digits = (digits + modelData).slice(0, 6);
                        }
                    }
                }
            }
        }

        SettingsButton {
            y: 516
            width: 672
            fontFamily: fontMedium
            label: "Start"
            textColor: "#4cd07d"
            enabled: keypadSeconds > 0
            onClicked: {
                send("/clock/timer/start/" + keypadSeconds);
                page = "main";
                tab = "timers";
            }
        }
    }

    Item {
        y: 96
        width: 720; height: 624
        visible: page === "alarm"

        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 100
            width: 440; height: 100
            radius: 24
            color: "#161b22"
        }
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            Wheel {
                id: hourWheel
                count: 24
                fontFamily: fontDin
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: ":"
                font.family: fontDin
                font.pixelSize: 96
                color: "#e6ebf1"
            }
            Wheel {
                id: minuteWheel
                count: 60
                fontFamily: fontDin
            }
        }

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            y: 316
            spacing: 14
            Repeater {
                model: dayOrder
                Rectangle {
                    readonly property bool picked: editDays.indexOf(modelData) >= 0
                    width: 80; height: 80
                    radius: 40
                    color: picked ? "#2d5b40" : dayArea.pressed ? "#2f3743" : "#1d232c"
                    border.width: picked ? 2 : 0
                    border.color: "#4cd07d"
                    Text {
                        anchors.centerIn: parent
                        text: dayNames[modelData].charAt(0)
                        font.family: fontMedium
                        font.pixelSize: 28
                        color: picked ? "#e6ebf1" : "#9aa3b2"
                    }
                    MouseArea {
                        id: dayArea
                        anchors.fill: parent
                        onClicked: toggleDay(modelData)
                    }
                }
            }
        }

        Rectangle {
            x: 24; y: 412
            width: 672; height: 88
            radius: 24
            color: soundArea.pressed ? "#1d232c" : "#161b22"
            Text {
                x: 32
                anchors.verticalCenter: parent.verticalCenter
                text: "Sound"
                font.family: fontMedium
                font.pixelSize: 28
                color: "#e6ebf1"
            }
            Text {
                anchors.right: chevron.left
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                text: toneName(editTone)
                font.family: fontFamily
                font.pixelSize: 26
                color: "#9aa3b2"
            }
            Icon {
                id: chevron
                anchors.right: parent.right
                anchors.rightMargin: 24
                anchors.verticalCenter: parent.verticalCenter
                width: 28; height: 28
                rotation: 180
                kind: "back"
                color: "#7d8796"
            }
            MouseArea {
                id: soundArea
                anchors.fill: parent
                onClicked: page = "sound"
            }
        }

        Row {
            x: 24; y: 520
            spacing: 24
            SettingsButton {
                visible: editId !== 0
                width: 324
                fontFamily: fontMedium
                label: "Delete"
                textColor: "#eb5757"
                onClicked: {
                    send("/clock/alarm/" + editId + "/delete");
                    page = "main";
                }
            }
            SettingsButton {
                width: editId !== 0 ? 324 : 672
                fontFamily: fontMedium
                label: "Save"
                textColor: "#4cd07d"
                onClicked: saveAlarm()
            }
        }
    }

    ListView {
        x: 24; y: 96
        width: 672; height: 624
        visible: page === "sound"
        clip: true
        spacing: 10
        model: tones
        delegate: Rectangle {
            readonly property bool picked: editTone === modelData.key
            width: 672; height: 66
            radius: 20
            color: picked ? "#2d5b40" : toneArea.pressed ? "#1d232c" : "#161b22"
            border.width: picked ? 2 : 0
            border.color: "#4cd07d"
            Text {
                x: 28
                anchors.verticalCenter: parent.verticalCenter
                text: modelData.name
                font.family: fontMedium
                font.pixelSize: 26
                color: "#e6ebf1"
            }
            MouseArea {
                id: toneArea
                anchors.fill: parent
                onClicked: {
                    editTone = modelData.key;
                    send("/clock/preview/" + modelData.key);
                }
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        onPressed: {
            idle.restart();
            mouse.accepted = false;
        }
    }
}
