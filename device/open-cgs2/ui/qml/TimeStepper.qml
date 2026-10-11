import QtQuick 2.14

Row {
    id: stepper
    signal step(int minutes)
    spacing: 12

    Repeater {
        model: ["minus", "plus"]
        Item {
            width: 64; height: 64
            Rectangle {
                anchors.fill: parent
                radius: 32
                color: area.pressed ? "#2f3743" : "#232a34"
            }
            Icon {
                anchors.centerIn: parent
                width: 28; height: 28
                kind: modelData
                color: "#e6ebf1"
            }
            MouseArea {
                id: area
                anchors.fill: parent
                onClicked: stepper.step(modelData === "plus" ? 30 : -30)
            }
        }
    }
}
