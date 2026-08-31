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
        subtitle: "Every switch reveals through one of 22 built-in shader presets (wipes, blooms, ripples, glitches). The default rotates them randomly with no repeats; pin one preset in Ryoku Settings (wallpaper.transition_preset)."
    }

}
