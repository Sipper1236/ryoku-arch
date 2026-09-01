import QtQuick
import Quickshell
import Quickshell.Io
import "../.."
import "../../components"

Column {
    id: root
    property var colors
    property var saveConfigKey
    property var notifyThemeChanged
    property var _theme: ({ themeApps: true, gtkTheme: "adw", gnomeAccent: false })
    property string _reveal: "random"
    readonly property var _revealModel: [
        "random", "silk_fade", "diagonal_silk", "dream_curtain", "liquid_ribbon",
        "iris_open", "corner_bloom", "spotlight_rise", "wander_iris", "vignette_close",
        "celeste_veil", "comet_streak", "aurora_ripple", "starfall_bloom",
        "mosaic_swell", "ember_burn", "pond_wake", "glass_scatter", "signal_tear",
        "cathode_wink", "shutter_sweep", "wax_descent", "page_turn"
    ].map(function(p) {
        return { mode: p, label: p === "random" ? "Random"
            : p.replace(/_/g, " ").replace(/^./, function(c) { return c.toUpperCase() }) }
    })

    function _runHub(args) { Quickshell.execDetached(["ryoku-hub"].concat(args)) }

    FileView {
        id: themeFile
        path: Quickshell.env("HOME") + "/.config/ryoku/theme.json"
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                var d = JSON.parse(themeFile.text())
                root._theme = { themeApps: d.themeApps !== false, gtkTheme: d.gtkTheme || "adw", gnomeAccent: !!d.gnomeAccent }
            } catch (e) {}
        }
    }

    FileView {
        id: shellFile
        path: Quickshell.env("HOME") + "/.config/ryoku/shell.json"
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                var d = JSON.parse(shellFile.text())
                var v = (d.wallpaper && d.wallpaper.transition_preset) || "random"
                root._reveal = ("" + v).length ? ("" + v) : "random"
            } catch (e) {}
        }
    }

    width: parent ? parent.width : 0
    spacing: 8

    SettingsCard {
        colors: root.colors
        title: "Theme"
        width: parent.width

        RowDropdown {
            colors: root.colors
            title: "Scheme type"
            description: "Material 3 colour-generation algorithm."
            value: Config.matugenScheme.replace("scheme-", "")
            model: [
                { mode: "content",     label: "Content" },
                { mode: "expressive",  label: "Expressive" },
                { mode: "fidelity",    label: "Fidelity" },
                { mode: "fruit-salad", label: "Fruit salad" },
                { mode: "monochrome",  label: "Monochrome" },
                { mode: "neutral",     label: "Neutral" },
                { mode: "rainbow",     label: "Rainbow" },
                { mode: "tonal-spot",  label: "Tonal spot" },
                { mode: "vibrant",     label: "Vibrant" }
            ]
            onSelect: function(v) {
                var full = "scheme-" + v
                if (root.saveConfigKey) root.saveConfigKey("matugen.schemeType", full)
                if (root.notifyThemeChanged) root.notifyThemeChanged(full, Config.matugenMode, Config.matugenColorIndex)
            }
        }

        RowDropdown {
            colors: root.colors
            title: "Mode"
            description: "Dark or light theme."
            value: Config.matugenMode
            model: [
                { mode: "dark",  label: "Dark" },
                { mode: "light", label: "Light" }
            ]
            onSelect: function(v) {
                if (root.saveConfigKey) root.saveConfigKey("matugen.mode", v)
                if (root.notifyThemeChanged) root.notifyThemeChanged(Config.matugenScheme, v, Config.matugenColorIndex)
            }
        }

        RowDropdown {
            colors: root.colors
            title: "Source colour index"
            description: "Which palette slot to use as the seed colour."
            value: String(Config.matugenColorIndex)
            model: [
                { mode: "0", label: "0 (Primary)" },
                { mode: "1", label: "1" },
                { mode: "2", label: "2" },
                { mode: "3", label: "3" }
            ]
            onSelect: function(v) {
                var idx = parseInt(v, 10) | 0
                if (root.saveConfigKey) root.saveConfigKey("matugen.colorIndex", idx)
                if (root.notifyThemeChanged) root.notifyThemeChanged(Config.matugenScheme, Config.matugenMode, idx)
            }
        }

        RowTextInput {
            colors: root.colors
            title: "Contrast"
            description: "Matugen contrast. Range -1.0 to 1.0 (0 = standard, higher = more contrast)."
            value: Config.matugenContrast.toFixed(2)
            placeholder: "0.00"
            onCommit: function(v) {
                var n = parseFloat(v)
                if (isNaN(n)) n = 0
                n = Math.max(-1, Math.min(1, n))
                if (root.saveConfigKey) root.saveConfigKey("matugen.contrast", n)
                if (root.notifyThemeChanged) root.notifyThemeChanged(Config.matugenScheme, Config.matugenMode, Config.matugenColorIndex)
            }
        }
    }

    SettingsCard {
        colors: root.colors
        title: "App theming"
        kana: "配色"
        width: parent.width

        RowToggle {
            colors: root.colors
            title: "Theme apps"
            description: "Recolour GTK and app themes to match the scheme."
            checked: root._theme.themeApps
            onToggle: function(v) { root._theme.themeApps = v; root._runHub(["hypr", "theme-apps", v ? "on" : "off"]) }
        }

        RowDropdown {
            colors: root.colors
            title: "GTK theme"
            description: "Base GTK theme that apps build on."
            value: root._theme.gtkTheme
            model: [ { mode: "adw", label: "Adw" }, { mode: "adwaita", label: "Adwaita" }, { mode: "system", label: "System" } ]
            onSelect: function(v) { root._theme.gtkTheme = v; root._runHub(["hypr", "gtk-theme", v]) }
        }

        RowToggle {
            colors: root.colors
            title: "GNOME accent"
            description: "Sync the GNOME accent colour to the scheme."
            checked: root._theme.gnomeAccent
            onToggle: function(v) { root._theme.gnomeAccent = v; root._runHub(["hypr", "gnome-accent", v ? "on" : "off"]) }
        }

        RowAction {
            colors: root.colors
            title: "Ryoku signature"
            description: "Apply the Ryoku theme: frame bars, zero roundness, mono scheme."
            valueLabel: "APPLY"
            onClicked: root._runHub(["hypr", "ryoku-theme"])
        }
    }

    SettingsCard {
        colors: root.colors
        title: "Wallpaper"
        kana: "壁"
        width: parent.width

        RowDropdown {
            colors: root.colors
            title: "Reveal"
            description: "The transition played when the wallpaper changes."
            value: root._reveal
            model: root._revealModel
            onSelect: function(v) {
                root._reveal = v
                Quickshell.execDetached(["ryoku-shell", "call", "settings.patch",
                    JSON.stringify({ path: "wallpaper.transition_preset", value: v })])
            }
        }
    }
}
