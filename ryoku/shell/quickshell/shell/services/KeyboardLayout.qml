pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Hyprland

Item {
    id: root

    visible: false

    property string layout: ""
    property string variant: ""
    property string lastEvent: ""

    readonly property string signature: layout + (variant ? ":" + variant : "")

    Connections {
        target: Hyprland

        function onRawEvent(event) {
            if (event.name !== "activelayout")
                return

            root.lastEvent = event.data || ""

            const parts = (event.data || "").split(",")

            if (parts.length >= 2) {
                root.layout = parts[0].trim()
                root.variant = parts.slice(1).join(",").trim()

                console.log(
                    "[KeyboardLayout] layout:",
                    root.layout,
                    "variant:",
                    root.variant
                )
            }
        }
    }

    Component.onCompleted: {
        console.log("[KeyboardLayout] service loaded")
    }
}
