import QtQuick 2.14
import "../../qml"

Item {
    id: button
    property string label: "Talk"
    property string fontFamily
    readonly property bool held: area.pressed
    readonly property bool wide: width > height * 1.5
    signal send(string path)

    width: 100; height: 100

    Rectangle {
        anchors.fill: parent
        radius: Math.min(width, height) / 2
        gradient: Gradient {
            GradientStop { position: 0; color: button.held ? "#60cfbc" : "#479dba" }
            GradientStop { position: 1; color: button.held ? "#268779" : "#245777" }
        }
        scale: button.held ? 0.93 : 1
        Behavior on scale { NumberAnimation { duration: 80 } }
    }

    Icon {
        id: mic
        anchors.horizontalCenter: parent.horizontalCenter
        y: button.height * 0.18
        width: button.width * 0.38; height: button.height * 0.46
        visible: !button.wide
        kind: "mic"
    }
    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: mic.bottom
        anchors.topMargin: button.height * 0.06
        visible: !button.wide
        text: button.label
        font.family: button.fontFamily
        font.pixelSize: button.height * 0.14
        color: "white"
    }
    Row {
        anchors.centerIn: parent
        spacing: button.height * 0.16
        visible: button.wide
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            width: button.height * 0.32; height: button.height * 0.39
            kind: "mic"
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: button.label
            font.family: button.fontFamily
            font.pixelSize: button.height * 0.36
            color: "white"
        }
    }

    Timer {
        interval: 1000
        running: button.held
        repeat: true
        onTriggered: button.send("/intercom/talk")
    }
    MouseArea {
        id: area
        anchors.fill: parent
        preventStealing: true
        onPressed: button.send("/intercom/talk")
        onReleased: button.send("/intercom/release")
        onCanceled: button.send("/intercom/release")
    }
}
