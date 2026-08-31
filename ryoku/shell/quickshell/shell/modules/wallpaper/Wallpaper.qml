pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io

/**
 * Ryoku wallpaper topic bridge, one instance per monitor (the shell root's
 * per-screen scope constructs it with `screen`).
 *
 * Ryogami (the Rust wallpaper daemon) now owns the Background layer surface and
 * paints the wallpaper, transitions, and livewalls itself, so this component no
 * longer draws anything. It only subscribes to the `wallpaper` topic on
 * $XDG_RUNTIME_DIR/ryogami.sock -- one coalesced full-state frame
 * {default: ENTRY, outputs: {connector: ENTRY}} per revision -- and re-exposes
 * this output's entry (outputs[screen.name] or, absent an override, default) as
 * the wallpaper/depth urls and fit that the desktop's glass widget backdrop and
 * the depth-cutout foreground (modules/depth/DepthForeground) composite against.
 * Contract 08 sec 1, 2.6, 5, 7.
 *
 * The wallpaper switcher (modules/wallpaper/switcher) sets wallpapers through
 * ryogami, which feeds this same topic.
 */
Item {
    id: root

    // The monitor this bridge tracks, supplied by the shell root's per-screen
    // scope (contract 08 sec 7: hotplug adds a monitor -> a new instance here).
    required property var screen

    WallpaperFrame {
        id: frame
        screenName: root.screen ? root.screen.name : ""
    }
    readonly property string wallpaperUrl: frame.path.length > 0
        ? "file://" + frame.path + "?v=" + frame.revision : ""
    readonly property string depthUrl: frame.depth.length > 0
        ? "file://" + frame.depth + "?v=" + frame.depthRev : ""
    readonly property string fit: frame.fit
    // Ryogami paints the wallpaper out of band, so there is no in-shell decode
    // left to gate on: widgets simply wait for the first topic frame.
    readonly property bool reloadReady: frame.ready

    readonly property string sockPath: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/ryogami.sock"

    function apply(line: string): void {
        frame.apply(line);
    }

    // Subscribe once, then stream, mirroring the Tray/Clipboard singletons. A
    // second write would half-close the stream (daemon rule), so nothing else
    // writes here.
    Socket {
        id: sub
        path: root.sockPath
        parser: SplitParser {
            onRead: line => root.apply(line)
        }
        Component.onCompleted: connected = true
        onConnectionStateChanged: {
            if (connected) {
                write("subscribe wallpaper\n");
                flush();
            } else {
                retry.restart();
            }
        }
    }

    // Ryogami may be down when the shell loads (or restart under it); retry
    // quietly so the desktop rebinds once it returns.
    Timer {
        id: retry
        interval: 2000
        onTriggered: if (!sub.connected)
            sub.connected = true
    }
}
