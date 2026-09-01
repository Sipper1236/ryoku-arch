import QtQuick
import ".."
import "../.."
import "../../components"
import "../../services"

// The Edit / Tune settings tab: ryowalls grade + upscale on the focused
// wallpaper. A preview on top, then a combined colour grade (live debounced
// through grade.preview, Apply via grade.commit) and a waifu2x upscale job
// (upscale.start/status/cancel). sourcePath is the currently focused wallpaper.
Column {
    id: root
    property var colors
    property string sourcePath: ""

    property int _brightness: 0
    property int _contrast: 0
    property int _saturation: 0
    property int _warmth: 0
    property bool _vignette: false
    property string _previewPath: ""
    property int _previewVer: 0
    property bool _committing: false
    property bool _desktopView: false

    property int _upScale: 2
    property var _up: ({ running: false, phase: "", progress: 0, total: 0, verdict: null })

    readonly property bool _dirty: _brightness !== 0 || _contrast !== 0 || _saturation !== 0 || _warmth !== 0 || _vignette
    readonly property color _ink:    colors ? colors.surfaceText : "#e0e2e8"
    readonly property color _inkDim: colors ? colors.surfaceVariantText : "#c2c7cf"
    readonly property color _line:   colors ? colors.outline : Qt.rgba(1, 1, 1, 0.22)
    readonly property bool _isVideo: {
        var p = ("" + root.sourcePath).toLowerCase()
        return p.endsWith(".mp4") || p.endsWith(".webm") || p.endsWith(".mkv") || p.endsWith(".mov") || p.endsWith(".m4v") || p.endsWith(".avi")
    }
    Component.onCompleted: DaemonClient.paletteFrameStatus()

    width: parent ? parent.width : 0
    spacing: 12

    onSourcePathChanged: root._reset()
    function _reset() { _brightness = 0; _contrast = 0; _saturation = 0; _warmth = 0; _vignette = false; _previewPath = "" }

    Timer { id: debounce; interval: 220; onTriggered: root._doPreview() }
    function _schedulePreview() { if (root.sourcePath) debounce.restart() }
    function _doPreview() {
        DaemonClient.call("grade.preview", {
            input: root.sourcePath, brightness: root._brightness, contrast: root._contrast,
            saturation: root._saturation, warmth: root._warmth, vignette: root._vignette
        }, function(res, err) { if (!err && res && res.output) { root._previewPath = res.output; root._previewVer++ } })
    }
    function _apply() {
        if (!root.sourcePath || root._committing) return
        root._committing = true
        DaemonClient.call("grade.commit", {
            input: root.sourcePath, brightness: root._brightness, contrast: root._contrast,
            saturation: root._saturation, warmth: root._warmth, vignette: root._vignette
        }, function(res, err) { root._committing = false; if (!err) root._reset() })
    }

    Timer { id: upPoll; interval: 500; repeat: true; running: root._up.running; onTriggered: root._pollUpscale() }
    function _startUpscale() {
        if (!root.sourcePath) return
        DaemonClient.call("upscale.start", { input: root.sourcePath, scale: root._upScale }, function(res, err) {
            if (!err) root._up = { running: true, phase: "starting", progress: 0, total: 0, verdict: null }
        })
    }
    function _pollUpscale() { DaemonClient.call("upscale.status", {}, function(res, err) { if (!err && res) root._up = res }) }
    function _cancelUpscale() { DaemonClient.call("upscale.cancel", {}, function() {}) }

    // preview
    Rectangle {
        width: parent.width
        height: 300 * Config.uiScale
        radius: Style.radiusMedium
        color: Qt.rgba(0, 0, 0, 0.35)
        border.width: 1; border.color: root._line
        clip: true
        Image {
            anchors.fill: parent; anchors.margins: 1
            visible: !root._desktopView
            source: root._previewPath ? ("file://" + root._previewPath + "?v=" + root._previewVer)
                   : (root.sourcePath ? ("file://" + root.sourcePath) : "")
            fillMode: Image.PreserveAspectFit
            asynchronous: true; cache: false; smooth: true; mipmap: true
            sourceSize: Qt.size(1200, 700)
        }
        MockDesktop {
            anchors.fill: parent; anchors.margins: 1
            visible: root._desktopView && !!root.sourcePath && !root._isVideo
            pv: palPreview
            wallpaper: root._previewPath ? root._previewPath : root.sourcePath
        }
        Text {
            visible: !root.sourcePath
            anchors.centerIn: parent
            text: "no wallpaper focused"
            font.family: Style.fontFamily; font.pixelSize: 12; color: root._inkDim
        }
        Row {
            visible: !!root.sourcePath && !root._isVideo
            anchors.top: parent.top; anchors.right: parent.right; anchors.margins: 8
            spacing: 6
            FilterButton {
                colors: root.colors; icon: "\u{f02e9}"; tooltip: "Wallpaper"
                isActive: !root._desktopView
                onClicked: root._desktopView = false
            }
            FilterButton {
                colors: root.colors; icon: "\u{f0379}"; tooltip: "Desktop preview"
                isActive: root._desktopView
                onClicked: root._desktopView = true
            }
        }
        PalettePreview {
            id: palPreview
            visible: false
            source: root._isVideo ? "" : (root._previewPath ? root._previewPath : root.sourcePath)
        }
    }

    SettingsCard {
        colors: root.colors
        title: "Grade"; kana: "色"
        width: parent.width

        Column {
            width: parent.width
            spacing: Style.spacingMedium

            SettingsSlider { colors: root.colors; label: "Brightness"; value: root._brightness; min: -50; max: 50; onChange: function(v){ root._brightness = v; root._schedulePreview() } }
            SettingsSlider { colors: root.colors; label: "Contrast";   value: root._contrast;   min: -50; max: 50; onChange: function(v){ root._contrast = v; root._schedulePreview() } }
            SettingsSlider { colors: root.colors; label: "Saturation"; value: root._saturation; min: -100; max: 100; onChange: function(v){ root._saturation = v; root._schedulePreview() } }
            SettingsSlider { colors: root.colors; label: "Warmth";     value: root._warmth;     min: -100; max: 100; onChange: function(v){ root._warmth = v; root._schedulePreview() } }

            RowToggle { colors: root.colors; title: "Vignette"; description: "Darken the frame edges."; checked: root._vignette; onToggle: function(v){ root._vignette = v; root._schedulePreview() } }

            Item {
                width: parent.width; height: 32
                Row {
                    anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                    spacing: 8
                    FilterButton { colors: root.colors; label: "RESET"; register: false; onClicked: root._reset() }
                    FilterButton { colors: root.colors; label: root._committing ? "APPLYING\u2026" : "APPLY GRADE"; register: false; hasActiveColor: root._dirty && !root._committing; activeColor: root.colors ? root.colors.primary : Style.fallbackAccent; isActive: root._dirty && !root._committing; onClicked: root._apply() }
                }
            }
        }
    }

    SettingsCard {
        colors: root.colors
        title: "Upscale"; kana: "拡大"
        width: parent.width

        Item {
            width: parent.width; height: 40
            Text { anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; text: "Scale"; font.family: Style.fontFamily; font.pixelSize: 12; font.weight: Font.Medium; color: root._ink }
            Row {
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                Repeater {
                    model: [2, 4]
                    FilterButton { colors: root.colors; label: modelData + "x"; register: false; isActive: root._upScale === modelData; onClicked: root._upScale = modelData }
                }
            }
        }

        Item {
            width: parent.width; height: 44
            Rectangle {
                visible: root._up.running
                anchors { left: parent.left; right: upBtn.left; verticalCenter: parent.verticalCenter; rightMargin: 12 }
                height: 6; radius: 3
                color: Qt.rgba(root._ink.r, root._ink.g, root._ink.b, 0.12)
                Rectangle {
                    anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                    width: parent.width * ((root._up.total > 0) ? Math.max(0, Math.min(1, root._up.progress / root._up.total)) : 0.15)
                    radius: 3
                    color: root.colors ? root.colors.primary : Style.fallbackAccent
                    Behavior on width { NumberAnimation { duration: Style.animFast } }
                }
            }
            Text {
                visible: !root._up.running
                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                width: parent.width - upBtn.width - 12
                elide: Text.ElideRight
                text: (root._up.verdict && root._up.verdict.why) ? root._up.verdict.why : "waifu2x-ncnn-vulkan. Writes an upscaled copy beside the original."
                font.family: Style.fontFamily; font.pixelSize: 10; color: root._inkDim
            }
            Text {
                visible: root._up.running
                anchors { right: upBtn.left; rightMargin: 12; verticalCenter: parent.verticalCenter }
                text: (root._up.phase || "working") + (root._up.total > 0 ? ("  " + root._up.progress + "/" + root._up.total) : "")
                font.family: Style.fontFamilyCode; font.pixelSize: 9; color: root._inkDim
            }
            FilterButton {
                id: upBtn
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                colors: root.colors
                label: root._up.running ? "CANCEL" : "UPSCALE"
                register: false
                hasActiveColor: true
                activeColor: root._up.running ? (root.colors ? root.colors.error : "#e2342a") : (root.colors ? root.colors.primary : Style.fallbackAccent)
                isActive: true
                onClicked: root._up.running ? root._cancelUpscale() : root._startUpscale()
            }
        }
    }

    SettingsCard {
        visible: root._isVideo
        colors: root.colors
        title: "Palette frame"; kana: "採色"
        width: parent.width

        RowInput {
            colors: root.colors
            title: "Sample second"
            description: "Which second of a video clip matugen samples for the colour scheme. Re-derives the palette from that frame."
            value: Math.round(DaemonClient.paletteFrame)
            min: 0; max: 20; suffix: "s"
            onCommit: function(v) { DaemonClient.setPaletteFrame(v) }
        }
    }
}
