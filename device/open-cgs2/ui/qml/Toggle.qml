import QtQuick 2.14

Rectangle {
    id: toggle
    property bool on: false
    signal toggled()
    width: 92; height: 52
    radius: 26
    color: on ? "#4cd07d" : "#2a313c"
    Behavior on color { ColorAnimation { duration: 150 } }

    Rectangle {
        x: toggle.on ? parent.width - width - 6 : 6
        anchors.verticalCenter: parent.verticalCenter
        width: 40; height: 40
        radius: 20
        color: "#f4f6f9"
        Behavior on x { NumberAnimation { duration: 150 } }
    }
    MouseArea {
        anchors.fill: parent
        anchors.margins: -16
        onClicked: toggle.toggled()
    }
}
