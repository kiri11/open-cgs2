import QtQuick 2.14

Rectangle {
    id: view
    property var readings: []
    property int rating: -1
    property var levelNames: []
    property var levelColors: []
    property var pm: ({})
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal close()
    signal chart(string key)
    signal send(string path)

    readonly property bool pmKnown: pm.state !== undefined
    readonly property bool pmRunning: pm.state === "on" || pm.state === "warming" || pm.state === "fault"
    readonly property string pmStatus: ({
        on: "On",
        warming: "Starting…",
        fault: "Not responding",
        off: "Off",
        night: "Off for the night"
    })[pm.state] || "Unavailable"

    color: "#0b0d12"

    onVisibleChanged: if (visible) idle.restart()
    Timer { id: idle; interval: 120000; running: view.visible; onTriggered: view.close() }

    MouseArea {
        anchors.fill: parent
        onPressed: idle.restart()
    }

    Text {
        x: 24; y: 26
        text: "Air quality"
        font.family: fontMedium
        font.pixelSize: 30
        color: "#9aa3b2"
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
        x: 24; y: 64
        text: rating < 0 ? "—" : levelNames[rating]
        font.family: fontDin
        font.pixelSize: 128
        color: rating < 0 ? "#5b6470" : levelColors[rating]
    }

    Row {
        x: 24; y: 236
        spacing: 24

        Repeater {
            model: readings
            Rectangle {
                width: 208; height: 196
                radius: 24
                color: "#161b22"
                opacity: cardArea.pressed ? 0.6 : 1

                Text {
                    x: 20; y: 16
                    text: modelData.label
                    font.family: fontMedium
                    font.pixelSize: 24
                    color: "#9aa3b2"
                }
                Text {
                    id: number
                    x: 20; y: 46
                    visible: modelData.value !== null
                    text: modelData.value === null ? "" : Math.round(modelData.value)
                    font.family: fontDin
                    font.pixelSize: 84
                    color: "#e6ebf1"
                }
                Text {
                    anchors.left: number.right
                    anchors.leftMargin: 8
                    anchors.baseline: number.baseline
                    visible: number.visible
                    text: modelData.unit
                    font.family: fontFamily
                    font.pixelSize: 20
                    color: "#7d8796"
                }
                Text {
                    x: 20; y: 72
                    width: 168
                    visible: !number.visible
                    wrapMode: Text.WordWrap
                    text: modelData.note
                    font.family: fontFamily
                    font.pixelSize: 26
                    color: "#7d8796"
                }
                Text {
                    x: 20; y: 148
                    visible: modelData.level >= 0
                    text: modelData.level >= 0 ? levelNames[modelData.level] : ""
                    font.family: fontMedium
                    font.pixelSize: 24
                    color: modelData.level >= 0 ? levelColors[modelData.level] : "#7d8796"
                }
                MouseArea {
                    id: cardArea
                    anchors.fill: parent
                    onClicked: view.chart(modelData.key)
                }
            }
        }
    }

    Column {
        x: 24; y: 456
        spacing: 16

        Repeater {
            model: [
                { what: "power", label: "Particulate sensor", detail: pmStatus, on: pmRunning },
                { what: "night", label: "Off at night", detail: (pm.from || "") + " – " + (pm.until || ""), on: pm.night === true }
            ]
            Rectangle {
                width: 672; height: 104
                radius: 24
                color: "#161b22"
                opacity: pmKnown ? (rowArea.pressed ? 0.6 : 1) : 0.45

                Column {
                    x: 24
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Text {
                        text: modelData.label
                        font.family: fontMedium
                        font.pixelSize: 28
                        color: "#e6ebf1"
                    }
                    Text {
                        text: modelData.detail
                        font.family: fontFamily
                        font.pixelSize: 22
                        color: modelData.what === "power" && pm.state === "fault" ? "#eb5757" : "#7d8796"
                    }
                }
                Rectangle {
                    x: parent.width - width - 24
                    anchors.verticalCenter: parent.verticalCenter
                    width: 92; height: 52
                    radius: 26
                    color: modelData.on ? "#4cd07d" : "#2a313c"
                    Behavior on color { ColorAnimation { duration: 150 } }

                    Rectangle {
                        x: modelData.on ? parent.width - width - 6 : 6
                        anchors.verticalCenter: parent.verticalCenter
                        width: 40; height: 40
                        radius: 20
                        color: "#f4f6f9"
                        Behavior on x { NumberAnimation { duration: 150 } }
                    }
                }
                MouseArea {
                    id: rowArea
                    anchors.fill: parent
                    enabled: pmKnown
                    onClicked: {
                        idle.restart();
                        view.send("/pm/" + modelData.what + "/" + (modelData.on ? "off" : "on"));
                    }
                }
            }
        }
    }
}
