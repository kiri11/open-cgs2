import QtQuick 2.14

Rectangle {
    id: key
    property string text
    property string icon
    property string fontFamily
    property bool highlight: false
    signal pressed()
    width: 65; height: 70
    radius: 10
    color: area.pressed ? "#3a4350" : highlight ? "#2d5b40" : "#262d38"

    Text {
        anchors.centerIn: parent
        visible: key.icon === ""
        text: key.text
        font.family: key.fontFamily
        font.pixelSize: key.text.length > 1 ? 24 : 32
        color: "#e6ebf1"
    }
    Icon {
        anchors.centerIn: parent
        visible: key.icon !== ""
        width: 34; height: 34
        kind: key.icon
        color: "#e6ebf1"
    }
    MouseArea {
        id: area
        anchors.fill: parent
        onPressed: key.pressed()
    }
}
