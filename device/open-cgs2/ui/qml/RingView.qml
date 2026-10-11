import QtQuick 2.14

Rectangle {
    id: view
    property var ringing: null
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal send(string path)

    readonly property bool alarm: ringing !== null && ringing.kind === "alarm"

    color: "#0b0d12"

    MouseArea {
        anchors.fill: parent
    }

    Item {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 64
        width: 200; height: 200

        Rectangle {
            anchors.fill: parent
            radius: width / 2
            color: "transparent"
            border.width: 4
            border.color: "#f2c94c"
            SequentialAnimation on scale {
                running: view.visible
                loops: Animation.Infinite
                NumberAnimation { from: 0.86; to: 1.08; duration: 700; easing.type: Easing.InOutQuad }
                NumberAnimation { from: 1.08; to: 0.86; duration: 700; easing.type: Easing.InOutQuad }
            }
        }
        Icon {
            anchors.centerIn: parent
            width: 96; height: 96
            kind: alarm ? "bell" : "timer"
            color: "#f2c94c"
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 300
        text: alarm ? "Alarm" : "Time's up"
        font.family: fontMedium
        font.pixelSize: 34
        color: "#9aa3b2"
    }
    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 340
        text: ringing ? ringing.label : ""
        font.family: fontDin
        font.pixelSize: 150
        color: "#e6ebf1"
    }

    Row {
        x: 24; y: 584
        spacing: 24
        Repeater {
            model: [
                { label: alarm ? "Snooze 9 min" : "+1 min", primary: false },
                { label: "Stop", primary: true }
            ]
            Rectangle {
                width: 324; height: 112
                radius: 28
                color: modelData.primary ? (ringArea.pressed ? "#d4ac2c" : "#f2c94c") : (ringArea.pressed ? "#242b36" : "#161b22")
                Text {
                    anchors.centerIn: parent
                    text: modelData.label
                    font.family: fontMedium
                    font.pixelSize: 34
                    color: modelData.primary ? "#0b0d12" : "#e6ebf1"
                }
                MouseArea {
                    id: ringArea
                    anchors.fill: parent
                    onClicked: {
                        if (modelData.primary) {
                            send("/clock/stop");
                        } else if (alarm) {
                            send("/clock/snooze");
                        } else {
                            send("/clock/stop");
                            send("/clock/timer/start/60");
                        }
                    }
                }
            }
        }
    }
}
