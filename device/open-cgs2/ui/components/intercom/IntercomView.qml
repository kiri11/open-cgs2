import QtQuick 2.14
import "../../qml"

Rectangle {
    id: view
    property var intercom: null
    property bool localTalking: false
    property string fontFamily
    property string fontMedium
    property bool shown: false
    property var lastIncoming: []
    readonly property bool held: reply.held
    readonly property var peers: intercom ? intercom.peers : []
    readonly property var incoming: intercom ? intercom.incoming : []
    readonly property var links: intercom ? intercom.links : []
    readonly property bool talking: localTalking || held || (intercom !== null && intercom.talking)
    readonly property bool active: talking || incoming.length > 0
    signal send(string path)

    color: "#0b0d12"
    opacity: shown ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 180 } }

    onIncomingChanged: if (incoming.length > 0) lastIncoming = incoming.slice()
    onActiveChanged: {
        if (active) {
            linger.stop();
            shown = true;
        } else {
            linger.restart();
        }
    }
    Component.onCompleted: if (active) shown = true

    Timer { id: linger; interval: 1500; onTriggered: view.shown = false }
    MouseArea { anchors.fill: parent }

    Item {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 52
        width: 140; height: 140

        Rectangle {
            anchors.fill: parent
            radius: width / 2
            color: "transparent"
            border.width: 4
            border.color: "#60cfbc"
            SequentialAnimation on scale {
                running: view.active
                loops: Animation.Infinite
                NumberAnimation { from: 0.86; to: 1.1; duration: 600; easing.type: Easing.InOutSine }
                NumberAnimation { from: 1.1; to: 0.86; duration: 600; easing.type: Easing.InOutSine }
            }
        }
        Icon {
            anchors.centerIn: parent
            width: 54; height: 76
            kind: "mic"
            color: "#60cfbc"
        }
    }

    Text {
        x: 40; y: 218
        width: 640; height: 90
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.WordWrap
        font.family: view.fontMedium
        font.pixelSize: 38
        color: "#e6ebf1"
        text: talking ? "Talking to " + (peers.length === 1 ? peers[0] : "everyone") : lastIncoming.join(", ")
    }
    Text {
        x: 40; y: 310
        width: 640; height: 60
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        font.family: view.fontFamily
        font.pixelSize: 24
        color: "#9aa3b2"
        text: talking ? (incoming.length > 0 ? incoming.join(", ") + (incoming.length === 1 ? " is" : " are") + " also talking\nLet go to hear them" : "Hold to keep talking") : active ? "Speaking" : "Transmission ended"
    }

    Flickable {
        x: 40; y: 382
        width: 640; height: 112
        visible: view.talking
        contentHeight: linkList.height
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: linkList
            width: parent.width
            spacing: 8
            Repeater {
                model: view.links
                Text {
                    width: linkList.width
                    horizontalAlignment: Text.AlignHCenter
                    text: modelData.name + " · " + (modelData.state === "live" ? "Connected" : modelData.state === "failed" ? "Unreachable" : "Connecting…")
                    font.family: view.fontFamily
                    font.pixelSize: 24
                    color: modelData.state === "live" ? "#60cfbc" : modelData.state === "failed" ? "#eb5757" : "#9aa3b2"
                }
            }
        }
    }

    HoldToTalk {
        id: reply
        anchors.horizontalCenter: parent.horizontalCenter
        y: 510
        width: 164; height: 164
        visible: !view.localTalking
        label: reply.held ? "Talking" : "Reply"
        fontFamily: view.fontMedium
        onSend: view.send(path)
    }
    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 684
        visible: reply.visible
        text: view.talking ? "Release to listen" : "Hold to reply"
        font.family: view.fontFamily
        font.pixelSize: 22
        color: "#9aa3b2"
    }
}
