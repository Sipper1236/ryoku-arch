import QtQuick
import QtQuick.Effects
import ".."
import "../components"
import "../services"

// Specimen Edit: the ryowalls grade + upscale surface, folded into the picker.
// A framed modal over the focused wallpaper. Left is a live preview; right is a
// combined colour grade (brightness/contrast/saturation/warmth + vignette, all
// at once, debounced through grade.preview) with Apply (grade.commit), and a
// waifu2x upscale job (upscale.start/status/cancel) with progress.
Rectangle {
    id: panel

    property var colors
    property string sourcePath: ""
    signal closed()

    // grade axes (ryowalls GradeSheet ranges)
    property int _brightness: 0
    property int _contrast: 0
    property int _saturation: 0
    property int _warmth: 0
    property bool _vignette: false
    property string _previewPath: ""
    property int _previewVer: 0
    property bool _committing: false

    // upscale job
    property int _upScale: 2
    property var _up: ({ running: false, phase: "", progress: 0, total: 0, verdict: null })

    readonly property bool _dirty: _brightness !== 0 || _contrast !== 0 || _saturation !== 0 || _warmth !== 0 || _vignette
    readonly property color _ink:    colors ? colors.surfaceText : "#e0e2e8"
    readonly property color _inkDim: colors ? colors.surfaceVariantText : "#c2c7cf"
    readonly property color _line:   colors ? colors.outline : Qt.rgba(1, 1, 1, 0.22)
    readonly property color _paper:  colors ? colors.surface : "#101418"

    width: 980
    height: 620
    radius: Style.radiusLarge
    color: Qt.rgba(_paper.r, _paper.g, _paper.b, 0.98)
    border.width: 1
    border.color: _line
    layer.enabled: true
    layer.effect: MultiEffect { shadowEnabled: true; shadowBlur: 0.8; shadowVerticalOffset: 5; shadowColor: Qt.rgba(0, 0, 0, 0.5) }

    onSourcePathChanged: { panel._reset(); }

    function _reset() {
        _brightness = 0; _contrast = 0; _saturation = 0; _warmth = 0; _vignette = false
        _previewPath = ""
    }

    Timer { id: debounce; interval: 220; onTriggered: panel._doPreview() }
    function _schedulePreview() { if (panel.sourcePath) debounce.restart() }
    function _doPreview() {
        DaemonClient.call("grade.preview", {
            input: panel.sourcePath,
            brightness: panel._brightness, contrast: panel._contrast,
            saturation: panel._saturation, warmth: panel._warmth, vignette: panel._vignette
        }, function(res, err) {
            if (!err && res && res.output) { panel._previewPath = res.output; panel._previewVer++ }
        })
    }
    function _apply() {
        if (!panel.sourcePath || panel._committing) return
        panel._committing = true
        DaemonClient.call("grade.commit", {
            input: panel.sourcePath,
            brightness: panel._brightness, contrast: panel._contrast,
            saturation: panel._saturation, warmth: panel._warmth, vignette: panel._vignette
        }, function(res, err) { panel._committing = false; if (!err) panel.closed() })
    }

    Timer { id: upPoll; interval: 500; repeat: true; running: panel._up.running; onTriggered: panel._pollUpscale() }
    function _startUpscale() {
        if (!panel.sourcePath) return
        DaemonClient.call("upscale.start", { input: panel.sourcePath, scale: panel._upScale }, function(res, err) {
            if (!err) panel._up = { running: true, phase: "starting", progress: 0, total: 0, verdict: null }
        })
    }
    function _pollUpscale() {
        DaemonClient.call("upscale.status", {}, function(res, err) { if (!err && res) panel._up = res })
    }
    function _cancelUpscale() { DaemonClient.call("upscale.cancel", {}, function() {}) }

    // ── header ───────────────────────────────────────────────────────────
    Item {
        id: header
        anchors { top: parent.top; left: parent.left; right: parent.right; margins: Style.spacingXLarge }
        height: 22
        Row {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            Text { text: "//"; font.family: Style.fontFamilyMono; font.pixelSize: 11; color: Qt.rgba(panel._inkDim.r, panel._inkDim.g, panel._inkDim.b, 0.5); anchors.verticalCenter: parent.verticalCenter }
            Text { text: "EDIT_"; font.family: Style.fontFamily; font.pixelSize: 11; font.weight: Font.Medium; font.letterSpacing: 1.4; color: panel._inkDim; anchors.verticalCenter: parent.verticalCenter }
            Text { text: "調整"; font.family: Style.fontFamilyJp; font.pixelSize: 12; color: Qt.rgba(panel._inkDim.r, panel._inkDim.g, panel._inkDim.b, 0.5); anchors.verticalCenter: parent.verticalCenter }
        }
        Text {
            anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
            text: "\u2715"
            font.family: Style.fontFamilyMono; font.pixelSize: 14
            color: closeMa.containsMouse ? panel._ink : panel._inkDim
            MouseArea { id: closeMa; anchors.fill: parent; anchors.margins: -8; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: panel.closed() }
        }
    }
    Rectangle { anchors { top: header.bottom; left: parent.left; right: parent.right; leftMargin: Style.spacingXLarge; rightMargin: Style.spacingXLarge; topMargin: 8 } height: 1; color: Qt.rgba(panel._line.r, panel._line.g, panel._line.b, 0.55) }

    // ── body: preview | controls ─────────────────────────────────────────
    Row {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom; topMargin: 20; leftMargin: Style.spacingXLarge; rightMargin: Style.spacingXLarge; bottomMargin: Style.spacingXLarge }
        spacing: Style.spacingXLarge

        // live preview
        Rectangle {
            id: previewBox
            width: 440
            height: parent.height
            radius: Style.radiusMedium
            color: Qt.rgba(0, 0, 0, 0.35)
            border.width: 1; border.color: panel._line
            clip: true
            Image {
                id: prevImg
                anchors.fill: parent; anchors.margins: 1
                source: panel._previewPath ? ("file://" + panel._previewPath + "?v=" + panel._previewVer)
                       : (panel.sourcePath ? ("file://" + panel.sourcePath) : "")
                fillMode: Image.PreserveAspectFit
                asynchronous: true; cache: false; smooth: true; mipmap: true
                sourceSize: Qt.size(880, 1200)
            }
            Text {
                visible: prevImg.status !== Image.Ready
                anchors.centerIn: parent
                text: panel.sourcePath ? "\u2026" : "no specimen"
                font.family: Style.fontFamily; font.pixelSize: 12; color: panel._inkDim
            }
        }

        // controls column
        Column {
            width: parent.width - previewBox.width - parent.spacing
            spacing: Style.spacingLarge

            SettingsCard {
                colors: panel.colors
                title: "Grade"; kana: "色"
                width: parent.width

                Column {
                    width: parent.width
                    spacing: Style.spacingMedium

                    SettingsSlider { colors: panel.colors; label: "Brightness"; value: panel._brightness; min: -50; max: 50; onChange: function(v){ panel._brightness = v; panel._schedulePreview() } }
                    SettingsSlider { colors: panel.colors; label: "Contrast";   value: panel._contrast;   min: -50; max: 50; onChange: function(v){ panel._contrast = v; panel._schedulePreview() } }
                    SettingsSlider { colors: panel.colors; label: "Saturation"; value: panel._saturation; min: -100; max: 100; onChange: function(v){ panel._saturation = v; panel._schedulePreview() } }
                    SettingsSlider { colors: panel.colors; label: "Warmth";     value: panel._warmth;     min: -100; max: 100; onChange: function(v){ panel._warmth = v; panel._schedulePreview() } }

                    RowToggle { colors: panel.colors; title: "Vignette"; description: "Darken the frame edges."; checked: panel._vignette; onToggle: function(v){ panel._vignette = v; panel._schedulePreview() } }

                    Item {
                        width: parent.width; height: 32
                        Row {
                            anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                            spacing: 8
                            FilterButton { colors: panel.colors; label: "RESET"; register: false; onClicked: panel._reset() }
                            FilterButton { colors: panel.colors; label: panel._committing ? "APPLYING\u2026" : "APPLY GRADE"; register: false; hasActiveColor: panel._dirty && !panel._committing; activeColor: panel.colors ? panel.colors.primary : Style.fallbackAccent; isActive: panel._dirty && !panel._committing; onClicked: panel._apply() }
                        }
                    }
                }
            }

            SettingsCard {
                colors: panel.colors
                title: "Upscale"; kana: "拡大"
                width: parent.width

                Item {
                    width: parent.width; height: 40
                    Text { anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter; text: "Scale"; font.family: Style.fontFamily; font.pixelSize: 12; font.weight: Font.Medium; color: panel._ink }
                    Row {
                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                        spacing: 4
                        Repeater {
                            model: [2, 4]
                            FilterButton {
                                colors: panel.colors
                                label: modelData + "x"
                                register: false
                                isActive: panel._upScale === modelData
                                onClicked: panel._upScale = modelData
                            }
                        }
                    }
                }

                Item {
                    width: parent.width; height: 44
                    // progress track while running; verdict after
                    Rectangle {
                        visible: panel._up.running
                        anchors { left: parent.left; right: startBtn.left; verticalCenter: parent.verticalCenter; rightMargin: 12 }
                        height: 6; radius: 3
                        color: Qt.rgba(panel._ink.r, panel._ink.g, panel._ink.b, 0.12)
                        Rectangle {
                            anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                            width: parent.width * ((panel._up.total > 0) ? Math.max(0, Math.min(1, panel._up.progress / panel._up.total)) : 0.15)
                            radius: 3
                            color: panel.colors ? panel.colors.primary : Style.fallbackAccent
                            Behavior on width { NumberAnimation { duration: Style.animFast } }
                        }
                    }
                    Text {
                        visible: !panel._up.running
                        anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                        width: parent.width - startBtn.width - 12
                        elide: Text.ElideRight
                        text: (panel._up.verdict && panel._up.verdict.why) ? panel._up.verdict.why : "waifu2x-ncnn-vulkan. Writes an upscaled copy beside the original."
                        font.family: Style.fontFamily; font.pixelSize: 10; color: panel._inkDim
                    }
                    Text {
                        visible: panel._up.running
                        anchors { right: startBtn.left; rightMargin: 12; bottom: parent.verticalCenter; bottomMargin: 6 }
                        text: (panel._up.phase || "working") + (panel._up.total > 0 ? ("  " + panel._up.progress + "/" + panel._up.total) : "")
                        font.family: Style.fontFamilyMono; font.pixelSize: 9; color: panel._inkDim
                    }
                    FilterButton {
                        id: startBtn
                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                        colors: panel.colors
                        label: panel._up.running ? "CANCEL" : "UPSCALE"
                        register: false
                        hasActiveColor: true
                        activeColor: panel._up.running ? (panel.colors ? panel.colors.error : "#e2342a") : (panel.colors ? panel.colors.primary : Style.fallbackAccent)
                        isActive: true
                        onClicked: panel._up.running ? panel._cancelUpscale() : panel._startUpscale()
                    }
                }
            }
        }
    }
}
