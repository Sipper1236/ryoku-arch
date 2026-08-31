import QtQuick
import ".."
import "../.."
import "../../components"

Column {
    id: root
    property var colors
    property var saveConfigKey

    width: parent ? parent.width : 0
    spacing: 8

    readonly property var _shaderOptions: [
        { key: "random",             label: "Random" },
        { key: "bounce",             label: "Bounce" },
        { key: "chromatic-bloom",    label: "Chromatic Bloom" },
        { key: "circle-crop",        label: "Circle Crop" },
        { key: "colour-distance",    label: "Colour Distance" },
        { key: "crazy-parametric",   label: "Crazy Parametric" },
        { key: "crosswarp",          label: "Cross Warp" },
        { key: "crosshatch",         label: "Crosshatch" },
        { key: "directional",        label: "Directional" },
        { key: "directional-scaled", label: "Directional Scaled" },
        { key: "directional-wipe",   label: "Directional Wipe" },
        { key: "edge-transition",    label: "Edge Transition" },
        { key: "fadecolor",          label: "Fadecolor" },
        { key: "flyeye",             label: "Fly Eye" },
        { key: "glitch",             label: "Glitch" },
        { key: "glitch-displace",    label: "Glitch Displace" },
        { key: "heat-melt",          label: "Heat Melt" },
        { key: "ink-splash",         label: "Ink Splash" },
        { key: "inkwell-drop",       label: "Inkwell Drop" },
        { key: "iris",               label: "Iris" },
        { key: "liquid-ripple",      label: "Liquid Ripple" },
        { key: "morph",              label: "Morph" },
        { key: "mosaic-tumble",      label: "Mosaic Tumble" },
        { key: "overexposure",       label: "Overexposure" },
        { key: "parametric-glitch",  label: "Parametric Glitch" },
        { key: "perlin",             label: "Perlin" },
        { key: "pixelate",           label: "Pixelate" },
        { key: "pixelfade-wave",     label: "Pixelfade Wave" },
        { key: "plasma-flow",        label: "Plasma Flow" },
        { key: "polar-function",     label: "Polar Function" },
        { key: "polka-dots-curtain", label: "Polka Dots Curtain" },
        { key: "puzzle-right",       label: "Puzzle Right" },
        { key: "randomsquares",      label: "Randomsquares" },
        { key: "smoke",              label: "Smoke" },
        { key: "soft-warp-fade",     label: "Soft Warp Fade" },
        { key: "static-fade",        label: "Static Fade" },
        { key: "voronoi-shatter",    label: "Voronoi Shatter" },
        { key: "wave-warp",          label: "Wave Warp" },
        { key: "zoom-blur-pull",     label: "Zoom Blur Pull" }
    ]

    SettingsCard {
        colors: root.colors
        title: "Engine"
        subtitle: "Pixels are painted by the built-in path: the shell renders stills with reveal transitions, mpvpaper plays video and live items."

        SettingsRow {
            colors: root.colors
            title: "Wallpaper engine"
            description: "One engine, built in. External painters from the skwd lineage are gone."
            FilterButton {
                colors: root.colors
                label: "Built-in"
                skew: 8 * Config.uiScale; height: 26 * Config.uiScale
                isActive: true
            }
        }

        SettingsRow {
            colors: root.colors
            title: "Fill mode"
            description: "How the wallpaper is fitted to the screen. Applies to images and videos."
            Row {
                spacing: 4
                Repeater {
                    model: [
                        { key: "fill",    label: "Fill" },
                        { key: "fit",     label: "Fit" },
                        { key: "stretch", label: "Stretch" },
                        { key: "center",  label: "Center" },
                        { key: "tile",    label: "Tile" }
                    ]
                    FilterButton {
                        colors: root.colors
                        label: modelData.label
                        skew: 8 * Config.uiScale; height: 26 * Config.uiScale
                        isActive: Config.fillMode === modelData.key
                        onClicked: Config.saveKey("display.fillMode", modelData.key)
                    }
                }
            }
        }
    }

    SettingsCard {
        colors: root.colors
        title: "Transitions"
        subtitle: "The 38 skwd shader transitions, rendered by the shell on every switch. Random rotates them with no repeats; picking a shader pins it. The shell's own 22 reveal presets stay reachable by setting transition.shader to \"ryoku\"."

        RowToggle {
            colors: root.colors
            title: "Enable transitions"
            description: "Animate wallpaper switches; off means a plain cut."
            checked: Config.transitionEnabled
            onToggle: function(v) { Config.saveKey("transition.enabled", v) }
        }

        RowInput {
            colors: root.colors
            title: "Duration (ms)"
            description: "Transition length in milliseconds."
            value: Config.transitionDurationMs
            min: 100; max: 10000
            onCommit: function(v) { Config.saveKey("transition.durationMs", v) }
        }

        RowToggle {
            colors: root.colors
            title: "Random shader per transition"
            description: "Pick a different shader for every transition."
            checked: Config.transitionShader === "random"
            onToggle: function(v) {
                if (v) {
                    if (Config.transitionShader !== "random" && root.saveConfigKey)
                        root.saveConfigKey("transition.lastShader", Config.transitionShader)
                    if (root.saveConfigKey) root.saveConfigKey("transition.shader", "random")
                } else {
                    var fallback = (Config._data.transition && Config._data.transition.lastShader) || "morph"
                    if (fallback === "random") fallback = "morph"
                    if (root.saveConfigKey) root.saveConfigKey("transition.shader", fallback)
                }
            }
        }

        ShaderPicker {
            colors: root.colors
            model: root._shaderOptions.filter(function(s) { return s.key !== "random" })
            value: Config.transitionShader
            enabled: Config.transitionEnabled && Config.transitionShader !== "random"
            opacity: (Config.transitionEnabled && Config.transitionShader !== "random") ? 1.0 : 0.4
            onSelected: function(key) { if (root.saveConfigKey) root.saveConfigKey("transition.shader", key) }
        }
    }

}
