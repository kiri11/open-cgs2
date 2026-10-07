import QtQuick 2.14
import "../../qml"

Rectangle {
    id: overlay
    property var app
    property var config
    readonly property string phase: app.phase
    color: "#0b0d12"
    opacity: phase === "idle" ? 0 : 0.97
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 180 } }

    MouseArea {
        anchors.fill: parent
        onClicked: overlay.app.post("/dismiss")
    }

    Orb {
        id: big
        anchors.horizontalCenter: parent.horizontalCenter
        y: 44
        width: 170; height: 170
        phase: overlay.phase
        onClicked: overlay.app.post("/assist")
    }

    Text {
        id: status
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: big.bottom
        anchors.topMargin: 22
        text: phase === "listening" ? "Listening…" : phase === "thinking" ? "Thinking…" : phase === "error" ? (app.st.assist.error || "") : ""
        font.family: app.fontMedium
        font.pixelSize: 28
        color: phase === "error" ? "#eb5757" : "#9aa3b2"
    }

    Text {
        id: heard
        anchors.top: status.bottom
        anchors.topMargin: 12
        x: 40; width: 640
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        maximumLineCount: 3
        elide: Text.ElideRight
        text: app.st.assist.heard ? "“" + app.st.assist.heard + "”" : ""
        font.family: app.fontFamily
        font.italic: true
        font.pixelSize: 28
        color: "#8a94a3"
    }

    Flickable {
        anchors.top: heard.bottom
        anchors.topMargin: 20
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 28
        x: 40; width: 640
        contentHeight: reply.height
        clip: true
        onContentHeightChanged: if (contentHeight > height) contentY = contentHeight - height

        Text {
            id: reply
            width: parent.width
            horizontalAlignment: lineCount > 1 ? Text.AlignLeft : Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: overlay.app.st.assist.reply || ""
            font.family: overlay.app.fontFamily
            font.pixelSize: 34
            lineHeight: 1.15
            color: "#eef2f6"
        }
    }
}
