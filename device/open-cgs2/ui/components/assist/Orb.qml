import QtQuick 2.14
import "../../qml"

Item {
    id: orb
    property string phase: "idle"
    property string label
    property string fontFamily
    signal clicked()

    width: 200
    height: 200

    Rectangle {
        id: ring
        anchors.centerIn: parent
        width: orb.width
        height: width
        radius: width / 2
        color: "transparent"
        border.color: "#b9a2ff"
        border.width: 6
        opacity: 0
    }

    SequentialAnimation {
        running: orb.phase === "listening"
        loops: Animation.Infinite
        onStopped: { ring.opacity = 0; ring.scale = 1 }
        ParallelAnimation {
            NumberAnimation { target: ring; property: "scale"; from: 1; to: 1.3; duration: 900; easing.type: Easing.OutCubic }
            NumberAnimation { target: ring; property: "opacity"; from: 0.9; to: 0; duration: 900 }
        }
    }

    Rectangle {
        id: body
        anchors.fill: parent
        radius: width / 2
        gradient: Gradient {
            GradientStop { position: 0; color: orb.phase === "error" ? "#c0504d" : "#9474ff" }
            GradientStop { position: 1; color: orb.phase === "error" ? "#7d2f2d" : "#5638cf" }
        }
        scale: area.pressed ? 0.93 : 1
        Behavior on scale { NumberAnimation { duration: 80 } }

        SequentialAnimation on opacity {
            running: orb.phase === "speaking"
            loops: Animation.Infinite
            onStopped: body.opacity = 1
            NumberAnimation { from: 1; to: 0.7; duration: 600; easing.type: Easing.InOutSine }
            NumberAnimation { from: 0.7; to: 1; duration: 600; easing.type: Easing.InOutSine }
        }
    }

    Canvas {
        id: spinner
        anchors.fill: parent
        anchors.margins: -10
        visible: orb.phase === "thinking"
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            ctx.strokeStyle = "#d8ccff";
            ctx.lineWidth = 6;
            ctx.lineCap = "round";
            ctx.beginPath();
            ctx.arc(width / 2, height / 2, width / 2 - 4, 0, Math.PI * 0.6);
            ctx.stroke();
        }
        RotationAnimator on rotation {
            running: spinner.visible
            from: 0; to: 360
            duration: 1000
            loops: Animation.Infinite
        }
    }

    Canvas {
        id: mic
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: orb.label ? -orb.height * 0.08 : 0
        width: orb.width * (orb.label ? 0.27 : 0.36)
        height: width * 1.4
        onPaint: {
            var ctx = getContext("2d");
            var w = width, h = height, lw = w * 0.11;
            ctx.reset();
            ctx.fillStyle = "white";
            ctx.strokeStyle = "white";
            ctx.lineWidth = lw;
            ctx.lineCap = "round";
            var cw = w * 0.46, cx = (w - cw) / 2, ch = h * 0.58;
            ctx.beginPath();
            ctx.moveTo(cx, cw / 2);
            ctx.arc(w / 2, cw / 2, cw / 2, Math.PI, 0);
            ctx.lineTo(cx + cw, ch - cw / 2);
            ctx.arc(w / 2, ch - cw / 2, cw / 2, 0, Math.PI);
            ctx.closePath();
            ctx.fill();
            ctx.beginPath();
            ctx.arc(w / 2, ch * 0.62, w / 2 - lw / 2, 0, Math.PI);
            ctx.stroke();
            ctx.beginPath();
            ctx.moveTo(w / 2, ch * 0.62 + w / 2 - lw / 2);
            ctx.lineTo(w / 2, h - lw / 2);
            ctx.stroke();
        }
    }

    Text {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: mic.bottom
        anchors.topMargin: orb.height * 0.04
        visible: orb.label !== ""
        text: orb.label
        font.family: orb.fontFamily
        font.pixelSize: orb.height * 0.14
        color: "white"
    }

    MouseArea {
        id: area
        anchors.fill: parent
        onClicked: orb.clicked()
    }
}
