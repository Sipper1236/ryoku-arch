import QtQuick
import Quickshell
import "../kit"
import "../../modules"
import Ryoku.Ui
import Ryoku.Ui.Singletons

// Community route (有志): every bar plugin installed from outside Ryoku, kept
// apart from the shipped set so a widget someone else wrote is never mixed in
// with the built-ins. The same CcWidgetList sheet as Widgets, so a community
// widget is shown, tuned and coloured exactly like a built-in; each row also
// names its author and can REMOVE the plugin. Below the sheet, the two ways in:
// a git URL handed to `ryoku plugin add --bar`, and Ryostore's plugin shelf.
Item {
    id: page
    property var root: null
    property var cc: null
    readonly property var tk: page.cc ? page.cc.tokens : null
    readonly property real colW: page.width
    implicitHeight: contentCol.implicitHeight

    property alias selId: clist.selId
    property string colorGid: ""
    property string colorLabel: ""
    readonly property bool colorSupported: !!(page.root && page.root.widgetHasFill)

    readonly property var communityEntries: {
        var c = (page.root && page.root.barCatalog) ? page.root.barCatalog : []
        var out = []
        for (var i = 0; i < c.length; i++)
            if (c[i].kind === "plugin" && c[i].official !== true) out.push(c[i])
        return out
    }

    // `ryoku plugin add` clones, validates and installs without running anything
    // from the plugin; --bar places it on the bar, which re-derives on the
    // plugins.json change. The field clears once the install is handed off.
    function addFromGit(url) {
        var u = String(url || "").trim()
        if (u === "") return
        Quickshell.execDetached(["ryoku", "plugin", "add", u, "--bar", "--yes"])
        gitField.text = ""
    }

    // what is still below the plate's edge; the stage reads it for the fade.
    readonly property real scrollRemaining: Math.max(0, flick.contentHeight - flick.height - flick.contentY)

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: contentCol.implicitHeight + (contentCol.implicitHeight > height && page.tk ? page.tk.tailPad : 0)
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: contentCol
            width: page.colW
            spacing: page.tk ? page.tk.sectionGap : 24

            Entrance {
                width: page.colW
                index: 0
                visible: page.communityEntries.length > 0
                CcWidgetList {
                    id: clist
                    width: page.colW
                    title: "INSTALLED"
                    kana: "\u5c0e\u5165"
                    root: page.root
                    tk: page.tk
                    entries: page.communityEntries
                    colorGid: page.colorGid
                    onColorRequested: (gid, label) => {
                        if (page.colorGid === gid) { page.colorGid = ""; page.colorLabel = "" }
                        else { page.colorGid = gid; page.colorLabel = label }
                    }
                }
            }

            // nothing installed yet: say so plainly, in the sheet's own frame.
            Entrance {
                width: page.colW
                index: 0
                visible: page.communityEntries.length === 0
                Rectangle {
                    width: page.colW
                    height: emptyCol.implicitHeight + (page.tk ? page.tk.pad * 2 : 48)
                    radius: Tokens.radius
                    color: "transparent"
                    border.width: Tokens.border
                    border.color: Tokens.line
                    Column {
                        id: emptyCol
                        anchors.centerIn: parent
                        width: parent.width - (page.tk ? page.tk.pad * 2 : 48)
                        spacing: 2
                        UiText {
                            text: I18n.tr("No community widgets yet")
                            color: Tokens.ink
                            font.family: Tokens.ui
                            font.pixelSize: Tokens.fRow
                            font.weight: Font.Medium
                        }
                        UiText {
                            width: parent.width
                            text: I18n.tr("A bar plugin from a git repo or Ryostore lands here.")
                            color: Tokens.inkMuted
                            font.family: Tokens.ui
                            font.pixelSize: Tokens.fSmall
                            wrapMode: Text.WordWrap
                        }
                    }
                }
            }

            Entrance {
                width: page.colW
                index: 1
                SettingCard {
                    width: page.colW
                    title: "ADD"
                    kana: "\u8ffd\u52a0"
                    collapsible: false

                    SettingRow {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        block: true
                        label: I18n.tr("From a git repo")
                        desc: I18n.tr("Cloned, checked, then placed on the bar")
                        Row {
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: page.tk ? page.tk.gap : 12
                            Field {
                                id: gitField
                                width: parent.width - addBtn.width - parent.spacing
                                tabular: true
                                placeholder: "https://github.com/someone/ryoku-widget"
                                onCommitted: (v) => page.addFromGit(v)
                            }
                            Btn {
                                id: addBtn
                                anchors.verticalCenter: parent.verticalCenter
                                text: I18n.tr("ADD")
                                primary: true
                                onAct: page.addFromGit(gitField.text)
                            }
                        }
                    }
                    SettingRow {
                        anchors.left: parent.left
                        anchors.right: parent.right
                        divider: true
                        controlWidth: 150
                        label: I18n.tr("From Ryostore")
                        desc: I18n.tr("The curated shelf")
                        Btn {
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            text: I18n.tr("OPEN RYOSTORE")
                            onAct: {
                                Quickshell.execDetached(["ryostore", "open", "plugins"])
                                if (page.cc) page.cc.close()
                            }
                        }
                    }
                }
            }
        }
    }

    CcScrollRail { root: page.root; tk: page.tk; flick: flick; z: 5 }

    Component {
        id: colorPopComp
        CcWidgetColorPopover {
            root: page.root
            tk: page.tk
            gid: page.colorGid
            label: page.colorLabel
            onDismissed: { page.colorGid = ""; page.colorLabel = "" }
        }
    }
    Loader {
        anchors.fill: parent
        z: 60
        active: page.colorSupported && page.colorGid !== ""
        sourceComponent: colorPopComp
    }
}
