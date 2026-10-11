import QtQuick 2.14

PathView {
    id: wheel
    property int count
    property string fontFamily
    readonly property real step: height / 3
    width: 180; height: 300
    model: count
    pathItemCount: 5
    preferredHighlightBegin: 0.5
    preferredHighlightEnd: 0.5
    highlightRangeMode: PathView.StrictlyEnforceRange
    snapMode: PathView.SnapToItem
    clip: true

    path: Path {
        startX: wheel.width / 2; startY: -wheel.step
        PathAttribute { name: "s"; value: 0.5 }
        PathLine { x: wheel.width / 2; y: wheel.height / 2 }
        PathAttribute { name: "s"; value: 1 }
        PathLine { x: wheel.width / 2; y: wheel.height + wheel.step }
        PathAttribute { name: "s"; value: 0.5 }
    }
    delegate: Text {
        width: wheel.width; height: wheel.step
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        text: (index < 10 ? "0" : "") + index
        font.family: wheel.fontFamily
        font.pixelSize: 120
        scale: PathView.s || 0.5
        opacity: 0.25 + ((PathView.s || 0.5) - 0.5) * 1.5
        color: "#e6ebf1"
    }
}
