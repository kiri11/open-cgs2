import QtQuick 2.14

Rectangle {
    id: button
    property string label
    property string fontFamily
    property color textColor: "#e6ebf1"
    signal clicked()
    height: 88
    radius: 24
    color: area.pressed ? "#242b36" : "#1d232c"
    opacity: enabled ? 1 : 0.45

    Text {
        anchors.centerIn: parent
        text: button.label
        font.family: button.fontFamily
        font.pixelSize: 28
        color: button.textColor
    }
    MouseArea {
        id: area
        anchors.fill: parent
        onClicked: button.clicked()
    }
}
