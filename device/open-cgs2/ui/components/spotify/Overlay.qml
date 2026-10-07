import QtQuick 2.14
import "../../qml"

MusicView {
    id: view
    property var app
    property var config
    property int libraryV: -1
    readonly property var state: app.st.music || ({})
    opacity: app.modal === "spotify" ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 150 } }
    music: state
    fontFamily: app.fontFamily
    fontMedium: app.fontMedium
    onClose: app.modal = ""
    onSend: app.post(path)

    onStateChanged: {
        var v = state.library_v;
        if (v === undefined || v === libraryV)
            return;
        app.get("/library", function(lib) {
            if (!lib)
                return;
            playlists = lib.playlists;
            recent = lib.recent;
            mixes = lib.mixes || [];
            hasRecent = lib.recent_supported === true;
            libraryV = v;
        });
    }
}
