import QtQuick
import "../modules"
import "kit/Routes.js" as Routes
import Ryoku.Ui
import Ryoku.Ui.Singletons
import shell.services

// QS Bar Settings' left rail: the masthead, the four routes as one flat group,
// and a search row at the foot. The panel is about one thing, the bar, so there
// are no BAR/DESK/SHELL section headers to navigate: four routes, always one
// click away. Latin names the route, kanji seals it. The active route takes a
// bone plate with a `//` lead, the desktop's only emphasis; nothing here is
// coloured except the 力 seal.
Item {
    id: rail

    property var root
    property var tk
    property string current: ""
    signal chose(string id)
    signal searchRequested()
    signal hubRequested()

    // The plate takes its height from whichever is taller, the rail or the page:
    // masthead + the four routes + the search row + the printed foot, each with
    // the gap it actually sits in.
    implicitHeight: rail.tk
        ? rail.tk.headH + nav.implicitHeight + rail.tk.gap * 2
          + foot.height + margin.height + rail.tk.pad * 2
        : 560

    // the register sheet rides behind the rail only, exactly as the Hub's does:
    // the chrome carries the print texture, the content plate stays clean paper.
    Reg { anchors.fill: parent }

    // ── masthead ─────────────────────────────────────────────────────────────
    Item {
        id: masthead
        anchors { top: parent.top; left: parent.left; right: parent.right }
        height: rail.tk.headH

        Row {
            anchors.left: parent.left
            anchors.leftMargin: rail.tk.pad
            anchors.verticalCenter: parent.verticalCenter
            spacing: rail.tk.gap / 2

            UiText {
                anchors.verticalCenter: parent.verticalCenter
                text: "\u529b"
                color: Tokens.sun
                font.family: Tokens.jp
                font.pixelSize: Tokens.fBody
            }
            UiText {
                anchors.verticalCenter: parent.verticalCenter
                text: "RYOKU"
                color: Tokens.ink
                font.family: Tokens.mono
                font.pixelSize: Tokens.fMicro
                font.letterSpacing: Tokens.trackMark
                font.weight: Font.DemiBold
            }
            UiText {
                anchors.verticalCenter: parent.verticalCenter
                text: "QS BAR SETTINGS"
                color: Tokens.inkFaint
                font.family: Tokens.mono
                font.pixelSize: Tokens.fTiny
                font.letterSpacing: Tokens.trackLabel
            }
        }
        Rectangle {
            anchors { bottom: parent.bottom; left: parent.left; right: parent.right }
            height: 1
            color: Tokens.lineSoft
        }
    }

    // ── the four routes, one flat group ──────────────────────────────────────
    Column {
        id: nav
        anchors {
            top: masthead.bottom; topMargin: rail.tk.gap
            left: parent.left; right: parent.right
        }
        spacing: 4

        Repeater {
            model: Routes.ROUTES

            delegate: Rectangle {
                id: item
                required property var modelData
                readonly property bool on: rail.current === item.modelData.id

                width: nav.width - rail.tk.gap * 2
                x: rail.tk.gap
                height: rail.tk.navH
                radius: Tokens.radius
                color: item.on ? Tokens.bone : (ma.containsMouse ? Tokens.tint5 : "transparent")
                Behavior on color { ColorAnimation { duration: Tokens.snap } }

                Row {
                    anchors.left: parent.left
                    anchors.leftMargin: rail.tk.gap
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: rail.tk.gap / 2

                    // the `//` lead: the desktop's mark of the live surface,
                    // printed only on the selected route.
                    UiText {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: item.on
                        text: "//"
                        color: Tokens.inkOnBoneDim
                        font.family: Tokens.mono
                        font.pixelSize: Tokens.fMicro
                    }
                    UiText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: I18n.tr(item.modelData.label)
                        color: item.on ? Tokens.inkOnBone : Tokens.inkDim
                        font.family: Tokens.ui
                        font.pixelSize: Tokens.fBody
                        font.weight: item.on ? Font.DemiBold : Font.Normal
                    }
                }
                UiText {
                    anchors.right: parent.right
                    anchors.rightMargin: rail.tk.gap
                    anchors.verticalCenter: parent.verticalCenter
                    text: item.modelData.gloss || ""
                    color: item.on ? Tokens.inkOnBoneDim : Tokens.inkFaint
                    font.family: Tokens.jp
                    font.pixelSize: Tokens.fTiny
                }
                MouseArea {
                    id: ma
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: rail.chose(item.modelData.id)
                }
            }
        }
    }

    // ── foot: search, and the surface switch ─────────────────────────────────
    Rectangle {
        id: footLine
        anchors { bottom: foot.top; left: parent.left; right: parent.right }
        height: 1
        color: Tokens.lineSoft
    }
    Item {
        id: foot
        anchors { bottom: margin.top; left: parent.left; right: parent.right }
        height: hints.implicitHeight + rail.tk.gap

        Column {
            id: hints
            anchors.centerIn: parent
            width: parent.width - rail.tk.gap * 2
            spacing: 2

            Rectangle {
                width: parent.width
                height: rail.tk.navH
                radius: Tokens.radius
                color: searchMa.containsMouse ? Tokens.tint5 : "transparent"
                Behavior on color { ColorAnimation { duration: Tokens.snap } }

                UiText {
                    anchors.left: parent.left
                    anchors.leftMargin: rail.tk.gap
                    anchors.verticalCenter: parent.verticalCenter
                    text: I18n.tr("Search")
                    color: Tokens.inkFaint
                    font.family: Tokens.ui
                    font.pixelSize: Tokens.fSmall
                }
                Keycap {
                    anchors.right: parent.right
                    anchors.rightMargin: rail.tk.gap / 2
                    anchors.verticalCenter: parent.verticalCenter
                    text: "CTRL K"
                    us: 0.5
                    dark: !Tokens.light
                }
                MouseArea {
                    id: searchMa
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: rail.searchRequested()
                }
            }
            // Surface switch: sets which panel the bar's brand logo opens -- QS
            // Bar Settings, or the Super+Esc quick-settings sidebar. The lit
            // segment is the current target; picking the other retargets the logo.
            Item {
                width: parent.width
                height: navSeg.height
                Seg {
                    id: navSeg
                    anchors.horizontalCenter: parent.horizontalCenter
                    options: ["QS BAR", "QUICK SETTINGS"]
                    current: Config.launcherTarget === "quick" ? "QUICK SETTINGS" : "QS BAR"
                    onChose: (key) => Config.setLauncherTarget(key === "QUICK SETTINGS" ? "quick" : "studio")
                }
            }
        }
    }
    // The rail's last inch is genuinely empty, so it carries what the Hub's rail
    // carries: a way to the full Hub above a real Code 39 plate. It scans.
    Item {
        id: margin
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        anchors.leftMargin: rail.tk.pad
        anchors.rightMargin: rail.tk.pad
        anchors.bottomMargin: rail.tk.pad
        height: rail.tk.gap + edition.height + rail.tk.gap / 2 + plate.implicitHeight

        Rectangle {
            anchors { left: parent.left; right: parent.right; top: parent.top }
            height: 1
            color: Tokens.lineSoft
        }
        // The panel is the quick surface; the Hub is every setting. A persistent
        // way there, printed where the edition mark used to sit.
        Item {
            id: edition
            anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: rail.tk.gap }
            height: 20
            Row {
                id: hubRow
                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                spacing: Tokens.s2
                Pixel {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 14; height: 14
                    kind: "torii"
                }
                UiText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: I18n.tr("OPEN THE HUB")
                    color: hubMa.containsMouse ? Tokens.ink : Tokens.inkFaint
                    font.family: Tokens.mono
                    font.pixelSize: Tokens.fTiny
                    font.letterSpacing: Tokens.trackLabel
                }
                UiText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "\u276f"
                    color: Tokens.inkFaint
                    font.family: Tokens.mono
                    font.pixelSize: Tokens.fTiny
                    opacity: hubMa.containsMouse ? 1 : 0.5
                }
            }
            MouseArea {
                id: hubMa
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: rail.hubRequested()
            }
        }
        // A Code 39 plate cannot be clipped -- lose the stop bars and it stops
        // being a barcode -- so the module width is solved from the rail's own
        // inner width instead of being a constant that overflowed it and printed
        // across the divider into the page.
        Barcode {
            id: plate
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            text: "RYOKU"
            barHeight: 12
            unit: Math.max(0.7, Math.min(1.4, margin.width / ((plate.text.length + 2) * 16)))
        }
    }

    Rectangle {
        anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
        width: 1
        color: Tokens.lineSoft
    }
}
