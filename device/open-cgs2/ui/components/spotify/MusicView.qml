import QtQuick 2.14
import "../../qml"

Rectangle {
    id: view
    property var music: ({})
    property var playlists: []
    property var recent: []
    property var mixes: []
    // The Recent tab needs `spotify.recently_played`, which the stock Spotify integration lacks.
    property bool hasRecent: false
    property string tab: "playlists"
    property string fontFamily
    property string fontMedium
    signal close()
    signal send(string path)

    readonly property bool playing: music.state === "playing"
    readonly property bool hasActive: !!music.active
    readonly property color green: "#1ed760"

    color: "#0b0d12"

    function act(path) {
        idle.restart();
        send(path);
    }

    Timer {
        id: idle
        interval: 120000
        running: view.visible
        onTriggered: view.close()
    }

    MouseArea {
        anchors.fill: parent
        onPressed: idle.restart()
    }

    Column {
        id: speakers
        x: 124; y: 24
        spacing: 10
        readonly property int count: (music.speakers || []).length
        readonly property int rowHeight: Math.min(68, Math.floor((art.height - spacing * (count - 1)) / Math.max(count, 1)))

        Repeater {
            model: music.speakers || []
            Rectangle {
                readonly property bool isActive: modelData.id === music.active
                readonly property bool isTarget: !hasActive && modelData.id === music.selected
                readonly property bool lit: isActive && playing
                width: 148; height: speakers.rowHeight
                radius: height / 2
                color: lit ? green : "transparent"
                border.width: lit ? 0 : 2
                border.color: isActive || isTarget ? green : "#39414c"
                enabled: modelData.state !== "offline"
                opacity: enabled ? 1 : 0.35
                scale: tap.pressed ? 0.94 : 1
                Behavior on color { ColorAnimation { duration: 150 } }
                Behavior on scale { NumberAnimation { duration: 80 } }

                Text {
                    anchors.centerIn: parent
                    text: modelData.name
                    font.family: fontMedium
                    font.pixelSize: 26
                    color: lit ? "#0b0d12" : isActive || isTarget ? green : "#b8c0cb"
                }
                MouseArea {
                    id: tap
                    anchors.fill: parent
                    anchors.margins: -4
                    onClicked: act("/music/speaker/" + modelData.id)
                }
            }
        }
    }

    Rectangle {
        id: art
        x: 288; y: 24
        width: 224; height: 224
        radius: 24
        color: "#1a1f27"

        SpotifyLogo {
            anchors.centerIn: parent
            width: 96; height: 96
            visible: cover.status !== Image.Ready
        }
        Image {
            id: cover
            anchors.fill: parent
            source: hasActive && music.art ? music.art : ""
            sourceSize.width: 448
            sourceSize.height: 448
            fillMode: Image.PreserveAspectCrop
            asynchronous: true
        }
    }

    Column {
        x: 532; y: 80
        width: 172
        spacing: 8

        Text {
            width: parent.width
            text: hasActive ? (music.title || "—") : "Nothing playing"
            font.family: fontMedium
            font.pixelSize: 28
            color: "#eef2f6"
            wrapMode: Text.WordWrap
            maximumLineCount: 4
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: hasActive ? (music.artist || "") : "Pick a playlist below"
            font.family: fontFamily
            font.pixelSize: 22
            color: "#9aa3b2"
            wrapMode: Text.WordWrap
            maximumLineCount: 2
            elide: Text.ElideRight
        }
    }

    Rectangle {
        readonly property bool on: music.shuffle === true
        x: 124; y: 298
        width: 64; height: 64
        radius: 32
        color: on ? "#1d3326" : "#1a1f27"
        enabled: hasActive && typeof music.shuffle === "boolean"
        opacity: enabled ? 1 : 0.35
        scale: shuffleHit.pressed ? 0.9 : 1
        Behavior on scale { NumberAnimation { duration: 80 } }

        Icon {
            anchors.centerIn: parent
            width: 34; height: 34
            kind: "shuffle"
            color: parent.on ? green : "#e6ebf1"
        }
        MouseArea {
            id: shuffleHit
            anchors.fill: parent
            anchors.margins: -8
            onClicked: act("/music/shuffle")
        }
    }

    Row {
        x: 220; y: 268
        spacing: 36
        enabled: hasActive
        opacity: enabled ? 1 : 0.35

        Repeater {
            model: ["prev", "toggle", "next"]
            Rectangle {
                width: modelData === "toggle" ? 124 : 96
                height: width
                radius: width / 2
                anchors.verticalCenter: parent.verticalCenter
                color: modelData === "toggle" ? green : "#1a1f27"
                scale: hit.pressed ? 0.9 : 1
                Behavior on scale { NumberAnimation { duration: 80 } }

                Icon {
                    anchors.centerIn: parent
                    width: parent.width * 0.46; height: width
                    kind: modelData === "toggle" ? (playing ? "pause" : "play") : modelData
                    color: modelData === "toggle" ? "#0b0d12" : "#e6ebf1"
                }
                MouseArea {
                    id: hit
                    anchors.fill: parent
                    anchors.margins: -8
                    onClicked: act("/music/" + modelData)
                }
            }
        }
    }

    Item {
        id: volume
        x: 16; y: 16
        width: 88; height: 688
        readonly property bool known: hasActive && typeof music.volume === "number"
        property real local: 0
        property real sent: -1
        readonly property real shown: slide.pressed || hold.running ? local : known ? music.volume / 100 : 0
        enabled: known
        opacity: enabled ? 1 : 0.35

        function push() {
            var v = Math.round(local * 100);
            if (v !== sent) {
                sent = v;
                act("/music/volume/" + v);
            }
        }
        function step(d) {
            sent = -1;
            local = Math.max(0, Math.min(100, Math.round(shown * 100) + d)) / 100;
            hold.restart();
            idle.restart();
            if (!later.running)
                later.start();
        }

        Repeater {
            model: [{ d: 2, icon: "plus", y: 0 }, { d: -2, icon: "minus", y: 600 }]
            Rectangle {
                y: modelData.y
                width: 88; height: 88; radius: 44
                color: stepArea.pressed ? "#262d38" : "#1a1f27"
                scale: stepArea.pressed ? 0.93 : 1
                Behavior on scale { NumberAnimation { duration: 80 } }
                Icon {
                    anchors.centerIn: parent
                    width: 36; height: 36
                    kind: modelData.icon
                    color: "#e6ebf1"
                }
                MouseArea {
                    id: stepArea
                    anchors.fill: parent
                    anchors.margins: -8
                    onClicked: volume.step(modelData.d)
                    onPressAndHold: repeat.start()
                    onReleased: repeat.stop()
                    onCanceled: repeat.stop()
                    Timer { id: repeat; interval: 180; repeat: true; onTriggered: volume.step(modelData.d) }
                }
            }
        }
        Rectangle {
            id: track
            x: 38; y: 128
            width: 12; height: 432
            radius: 6
            color: "#1a1f27"

            Rectangle {
                y: parent.height * (1 - volume.shown)
                width: parent.width
                height: parent.height * volume.shown
                radius: 6
                color: green
            }
            Rectangle {
                y: parent.height * (1 - volume.shown) - height / 2
                anchors.horizontalCenter: parent.horizontalCenter
                width: slide.pressed ? 48 : 40; height: width
                radius: width / 2
                color: "#eef2f6"
                Behavior on width { NumberAnimation { duration: 80 } }
            }
        }
        MouseArea {
            id: slide
            x: 0; y: 104
            width: parent.width; height: 480
            preventStealing: true
            function set(my) {
                volume.local = Math.max(0, Math.min(1, 1 - (my + y - track.y) / track.height));
                idle.restart();
            }
            onPressed: { volume.sent = music.volume; set(mouse.y); throttle.start(); }
            onPositionChanged: set(mouse.y)
            onReleased: { throttle.stop(); volume.push(); hold.restart(); }
            onCanceled: { throttle.stop(); hold.restart(); }
        }
        Timer { id: throttle; interval: 300; repeat: true; onTriggered: volume.push() }
        Timer { id: later; interval: 300; onTriggered: volume.push() }
        Timer { id: hold; interval: 2000 }
    }

    Row {
        x: 124; y: 408
        spacing: 36

        Repeater {
            model: [{"id": "playlists", "name": "Playlists"}].concat(view.mixes.length ? [{"id": "mixes", "name": "Mixes"}] : [], view.hasRecent ? [{"id": "recent", "name": "Recent"}] : [])
            Item {
                readonly property bool chosen: view.tab === modelData.id
                width: label.width; height: 44

                Text {
                    id: label
                    text: modelData.name
                    font.family: fontMedium
                    font.pixelSize: 26
                    color: chosen ? "#eef2f6" : "#5b6470"
                }
                Rectangle {
                    y: 38
                    width: parent.width; height: 4
                    radius: 2
                    color: green
                    visible: chosen
                }
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -12
                    onClicked: {
                        view.tab = modelData.id;
                        grid.positionViewAtBeginning();
                        idle.restart();
                    }
                }
            }
        }
    }

    Item {
        x: 648; y: 20
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
            onClicked: view.close()
        }
    }

    GridView {
        id: grid
        x: 124; y: 464
        width: 580; height: 240
        cellWidth: 290; cellHeight: 92
        clip: true
        model: view.tab === "recent" ? recent : view.tab === "mixes" ? mixes : playlists
        boundsBehavior: Flickable.StopAtBounds
        onMovementStarted: idle.restart()

        delegate: Item {
            width: 290; height: 92
            readonly property bool current: modelData.uri === (view.tab === "recent" ? music.track : music.current)

            Rectangle {
                width: 278; height: 80
                radius: 18
                color: current ? "#1d3326" : "#1a1f27"
                border.width: current ? 2 : 0
                border.color: green
                scale: press.pressed ? 0.96 : 1
                Behavior on scale { NumberAnimation { duration: 80 } }

                Column {
                    x: 20
                    width: parent.width - 40
                    anchors.verticalCenter: parent.verticalCenter

                    Text {
                        width: parent.width
                        text: modelData.name
                        font.family: fontFamily
                        font.pixelSize: 24
                        color: "#e6ebf1"
                        wrapMode: Text.WordWrap
                        maximumLineCount: modelData.artist ? 1 : 2
                        elide: Text.ElideRight
                    }
                    Text {
                        width: parent.width
                        visible: !!modelData.artist
                        text: modelData.artist || ""
                        font.family: fontFamily
                        font.pixelSize: 20
                        color: "#9aa3b2"
                        elide: Text.ElideRight
                    }
                }
                MouseArea {
                    id: press
                    anchors.fill: parent
                    onClicked: act((view.tab === "recent" ? "/music/track/" : "/music/playlist/") + modelData.uri)
                }
            }
        }
    }

    Text {
        anchors.horizontalCenter: grid.horizontalCenter
        y: grid.y + 40
        visible: grid.count === 0
        text: view.tab === "recent" ? "Loading recent tracks…" : view.tab === "mixes" ? "No mixes" : "Loading playlists…"
        font.family: fontFamily
        font.pixelSize: 24
        color: "#5b6470"
    }
}
