import QtQuick 2.14

Row {
    property int level: 0
    spacing: 4

    Repeater {
        model: 4
        Rectangle {
            anchors.bottom: parent.bottom
            width: 7; height: 8 + index * 6
            radius: 2
            color: index < level ? "#e6ebf1" : "#3a424e"
        }
    }
}
