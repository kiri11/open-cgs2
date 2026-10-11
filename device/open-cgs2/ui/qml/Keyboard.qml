import QtQuick 2.14

// An on-screen keyboard for printable ASCII; some firmware lacks Qt's virtual keyboard.
Rectangle {
    id: keyboard
    property string fontFamily
    property string keySet: "abc"
    property bool shift: false
    signal typed(string text)
    signal erase()
    signal done()

    readonly property var keySets: ({
        abc: ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
        num: ["1234567890", "-/:;()$&@\"", ".,?!'"],
        sym: ["[]{}#%^*+=", "_\\|~<>`", ".,?!'"]
    })

    width: 720
    height: 4 * 76 + 20
    color: "#161b22"

    function label(ch) {
        return shift ? ch.toUpperCase() : ch;
    }

    Column {
        x: 6; y: 10
        spacing: 6

        Repeater {
            model: keySets[keySet]
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 6
                readonly property bool last: index === 2

                KeyCap {
                    fontFamily: keyboard.fontFamily
                    visible: last
                    width: 100
                    icon: keySet === "abc" ? "shift" : ""
                    text: keySet === "num" ? "#+=" : "123"
                    highlight: keySet === "abc" && shift
                    onPressed: {
                        if (keySet === "abc")
                            shift = !shift;
                        else
                            keySet = keySet === "num" ? "sym" : "num";
                    }
                }
                Repeater {
                    model: modelData.split("")
                    KeyCap {
                        fontFamily: keyboard.fontFamily
                        text: label(modelData)
                        onPressed: {
                            keyboard.typed(label(modelData));
                            shift = false;
                        }
                    }
                }
                KeyCap {
                    fontFamily: keyboard.fontFamily
                    visible: last
                    width: 100
                    icon: "erase"
                    onPressed: keyboard.erase()
                }
            }
        }
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: 6
            KeyCap {
                fontFamily: keyboard.fontFamily
                width: 140
                text: keySet === "abc" ? "123" : "abc"
                onPressed: keySet = keySet === "abc" ? "num" : "abc"
            }
            KeyCap {
                fontFamily: keyboard.fontFamily
                width: 388
                text: "space"
                onPressed: keyboard.typed(" ")
            }
            KeyCap {
                fontFamily: keyboard.fontFamily
                width: 168
                text: "Done"
                onPressed: keyboard.done()
            }
        }
    }
}
