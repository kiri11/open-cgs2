import QtQuick 2.14
import "../../qml"

Rectangle {
    id: view
    property var doorbell: null
    property bool screenOn: true
    property string fontFamily
    property string fontMedium
    signal send(string path)

    readonly property bool open: doorbell !== null
    readonly property bool ring: open && doorbell.ring
    readonly property Image front: frontA ? imageA : imageB
    readonly property bool shown: front.status === Image.Ready
    property bool frontA: true
    property bool loading: false
    property int frame: 0
    property double loadStarted: 0
    property double relayDownUntil: 0

    color: "#000000"
    visible: open

    onOpenChanged: {
        imageA.source = "";
        imageB.source = "";
        loading = false;
    }

    // Loads the next snapshot behind the one on screen and swaps them once it has decoded.
    // It comes through the relay shared with the other screens, asked again as soon as a frame arrives,
    // or straight from HA once a second for 30 s after the relay fails.
    function next() {
        if (!open || !screenOn || !doorbell.picture)
            return;
        var back = frontA ? imageB : imageA;
        if (loading) {
            if (Date.now() - loadStarted < 10000)
                return;
            if (back.relay)
                relayDownUntil = Date.now() + 30000;
        }
        loading = true;
        loadStarted = Date.now();
        frame++;
        back.relay = !!doorbell.relay && Date.now() >= relayDownUntil;
        back.source = (back.relay ? doorbell.relay : doorbell.picture) + "&frame=" + frame;
    }

    function loaded(image) {
        if (image.status === Image.Ready && image.source != "") {
            frontA = image === imageA;
            loading = false;
            if (image.relay)
                next();
        } else if (image.status === Image.Error) {
            loading = false;
            if (image.relay) {
                relayDownUntil = Date.now() + 30000;
                next();
            }
        }
    }

    Timer {
        interval: 1000
        repeat: true
        triggeredOnStart: true
        running: view.open && view.screenOn
        onTriggered: view.next()
    }

    MouseArea { anchors.fill: parent }

    Image {
        id: imageA
        anchors.fill: parent
        sourceSize.width: 720
        sourceSize.height: 720
        fillMode: Image.PreserveAspectFit
        property bool relay
        asynchronous: true
        cache: false
        visible: frontA
        onStatusChanged: view.loaded(imageA)
    }
    Image {
        id: imageB
        anchors.fill: parent
        sourceSize.width: 720
        sourceSize.height: 720
        fillMode: Image.PreserveAspectFit
        property bool relay
        asynchronous: true
        cache: false
        visible: !frontA
        onStatusChanged: view.loaded(imageB)
    }

    Text {
        anchors.centerIn: parent
        visible: !view.shown
        text: open && !doorbell.picture ? "No camera picture" : "Waking the camera…"
        font.family: fontFamily
        font.pixelSize: 28
        color: "#7d8796"
    }

    Rectangle {
        width: parent.width; height: 180
        gradient: Gradient {
            GradientStop { position: 0; color: "#ff000000" }
            GradientStop { position: 0.5; color: "#f0000000" }
            GradientStop { position: 1; color: "#00000000" }
        }
    }

    Row {
        x: 32; y: 24
        spacing: 16

        Icon {
            anchors.verticalCenter: parent.verticalCenter
            width: 52; height: 52
            visible: view.ring
            kind: "bell"
            color: "#f2c94c"
            SequentialAnimation on opacity {
                running: view.ring && view.visible
                loops: Animation.Infinite
                NumberAnimation { from: 1; to: 0.35; duration: 600; easing.type: Easing.InOutQuad }
                NumberAnimation { from: 0.35; to: 1; duration: 600; easing.type: Easing.InOutQuad }
            }
        }
        Column {
            Text {
                text: !open ? "" : view.ring ? "Someone's at the door" : doorbell.label + " detected"
                font.family: fontMedium
                font.pixelSize: 36
                color: view.ring ? "#f2c94c" : "#e6ebf1"
            }
            Text {
                text: open ? doorbell.name + " · " + Fmt.time(new Date(doorbell.at)) : ""
                font.family: fontFamily
                font.pixelSize: 24
                color: "#9aa3b2"
            }
        }
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
            onClicked: send("/doorbell/dismiss")
        }
    }
}
