import QtQuick
import ".."
import "../services"

// A wallpaper-grid-style preview tile, so the theme and rice grids read the
// same as the main image selection: a rounded card, a surface-tinted body, a
// hover/selected border in the accent, an optional bottom label strip and a
// top-left badge. The preview itself (an image, a palette, a mini desktop)
// goes in the `content` slot; small hover controls go in `overlay`. Mirrors
// the main grid's gridCardRect chrome (radius 6, 4px cell margin, 2px accent
// border on hover) so a card is a card everywhere in the picker.
Item {
    id: card

    property var colors
    property alias content: contentSlot.data
    property alias overlay: overlaySlot.data
    property string label: ""
    property string badge: ""
    property color badgeColor: colors ? colors.primary : Style.fallbackAccent
    property bool selected: false
    readonly property bool hovered: cardMouse.containsMouse
    property real cardRadius: 6

    signal clicked(var mouse)
    signal rightClicked(var mouse)

    Rectangle {
        id: frame
        anchors.fill: parent
        anchors.margins: 4
        radius: card.cardRadius
        color: "transparent"
        border.width: (card.hovered || card.selected) ? 2 : 0
        border.color: card.colors ? card.colors.primary : Style.fallbackAccent
        Behavior on border.width { NumberAnimation { duration: Style.animFast; easing.type: Easing.OutQuad } }

        Rectangle {
            id: body
            anchors.fill: parent
            anchors.margins: frame.border.width
            radius: Math.max(0, card.cardRadius - 1)
            clip: true
            color: card.colors ? Qt.rgba(card.colors.surface.r, card.colors.surface.g, card.colors.surface.b, 0.6)
                               : Qt.rgba(0.12, 0.14, 0.18, 0.6)

            // preview content (base layer)
            Item { id: contentSlot; anchors.fill: parent }

            // resting hairline so a card still reads as a tile when not hovered
            Rectangle {
                anchors.fill: parent
                radius: body.radius
                color: "transparent"
                visible: !(card.hovered || card.selected)
                border.width: 1
                border.color: card.colors ? Qt.rgba(card.colors.surfaceText.r, card.colors.surfaceText.g, card.colors.surfaceText.b, 0.12)
                                          : Qt.rgba(1, 1, 1, 0.1)
            }

            // bottom label strip
            Rectangle {
                visible: card.label !== ""
                anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                height: labelText.implicitHeight + 12
                color: Qt.rgba(0, 0, 0, 0.55)
                Text {
                    id: labelText
                    anchors.left: parent.left; anchors.right: parent.right
                    anchors.leftMargin: 8; anchors.rightMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    text: card.label
                    elide: Text.ElideRight
                    font.family: Style.fontFamily
                    font.pixelSize: 11 * Config.uiScale
                    font.weight: Font.Medium
                    color: card.colors ? card.colors.surfaceText : "#e0e2e8"
                }
            }

            // top-left badge
            Rectangle {
                visible: card.badge !== ""
                anchors.top: parent.top; anchors.left: parent.left; anchors.margins: 6
                width: badgeText.implicitWidth + 10
                height: badgeText.implicitHeight + 6
                radius: 3
                color: Qt.rgba(0, 0, 0, 0.6)
                Text {
                    id: badgeText
                    anchors.centerIn: parent
                    text: card.badge
                    font.family: Style.fontFamily
                    font.pixelSize: 8 * Config.uiScale
                    font.weight: Font.Bold
                    font.letterSpacing: 0.5
                    color: card.badgeColor
                }
            }

            MouseArea {
                id: cardMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onClicked: function(mouse) {
                    if (mouse.button === Qt.RightButton) card.rightClicked(mouse)
                    else card.clicked(mouse)
                }
            }

            // hover controls sit above the card's own click area
            Item { id: overlaySlot; anchors.fill: parent }
        }
    }
}
