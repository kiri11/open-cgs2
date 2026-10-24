import QtQuick 2.14
import "../../qml"

Rectangle {
    id: view
    property var call: null
    property double nowMs
    property string fontDin
    property string fontFamily
    property string fontMedium
    signal send(string path)

    readonly property int minutes: call ? Math.max(0, Math.ceil((call.start * 1000 - nowMs) / 60000)) : 0

    color: "#0b0d12"
    visible: call !== null

    MouseArea { anchors.fill: parent }

    Item {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 72
        width: 180; height: 180

        Rectangle {
            anchors.fill: parent
            radius: width / 2
            color: "transparent"
            border.width: 4
            border.color: "#4cd07d"
            SequentialAnimation on scale {
                running: view.visible
                loops: Animation.Infinite
                NumberAnimation { from: 0.88; to: 1.06; duration: 900; easing.type: Easing.InOutQuad }
                NumberAnimation { from: 1.06; to: 0.88; duration: 900; easing.type: Easing.InOutQuad }
            }
        }
        Icon {
            anchors.centerIn: parent
            width: 88; height: 88
            kind: "video"
            color: "#4cd07d"
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 290
        text: call ? "Call at " + Fmt.time(new Date(call.start * 1000)) : ""
        font.family: fontMedium
        font.pixelSize: 34
        color: "#9aa3b2"
    }
    Text {
        x: 48; y: 344
        width: parent.width - 96
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        maximumLineCount: 3
        elide: Text.ElideRight
        text: call ? call.summary : ""
        font.family: fontMedium
        font.pixelSize: 44
        lineHeight: 1.1
        color: "#e6ebf1"
    }
    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        y: 520
        text: minutes > 0 ? "in " + minutes + " min" : "now"
        font.family: fontDin
        font.pixelSize: 110
        color: "#4cd07d"
    }

    Item {
        x: 648; y: 24
        width: 48; height: 48

        Icon {
            anchors.centerIn: parent
            width: 30; height: 30
            kind: "close"
            color: "#9aa3b2"
        }
        MouseArea {
            anchors.fill: parent
            anchors.margins: -12
            onClicked: send("/call/dismiss")
        }
    }
}
