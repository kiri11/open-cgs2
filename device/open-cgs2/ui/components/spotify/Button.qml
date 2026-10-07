import QtQuick 2.14
import "../../qml"

Item {
    id: button
    property var app
    property var config
    width: 66; height: 66
    opacity: app.online ? 1 : 0.45
    scale: area.pressed ? 0.92 : 1
    Behavior on scale { NumberAnimation { duration: 80 } }

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: "transparent"
        border.width: 3
        border.color: "#1ed760"
        visible: (button.app.st.music || {}).state === "playing"
    }
    SpotifyLogo {
        anchors.centerIn: parent
        width: 54; height: 54
    }
    MouseArea {
        id: area
        anchors.fill: parent
        anchors.margins: -8
        enabled: button.app.online
        onClicked: {
            button.app.modal = "spotify";
            button.app.post("/music/open");
        }
    }
}
