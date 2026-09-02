# Writing a Ryoku shell plugin

A plugin is a small widget you write that the **user** drops into the Ryoku
desktop. You write the *content and the logic*. **Ryoku owns everything about how
it looks, moves, sizes, and where it lives.** That split is the whole point: a
plugin always looks and behaves like a native part of the shell, because the
shell - not you - draws the surface around it.

If you only read one thing, read **"Who does what"** below.

---

## Who does what

| You (the contributor) write... | Ryoku (the shell) handles for you... |
|---|---|
| The **logic**: fetch data, hold state, run commands (`service/Main.qml`). | **Where the widget lives** (frame popout, desktop widget) and letting the user choose and move it. |
| **One view** of your widget (`content/Widget.qml`) - the labels, buttons, grid, etc. | The **card / popout surface** behind your view: background, rounded corners, shadow, hairline. |
| A small **manifest** describing your plugin and its defaults. | **Dragging, resizing, the right-click menu, hover-to-open** - all the interaction. |
| A **settings schema** in your manifest (`metadata.settings`). | **Sizing**: the popout grows to fit your content; the desktop tile scales as the user resizes it. |
| Optionally, **shipped scripts/binaries** (`bin/`). | **Theming, motion, the brand look** (the "deck dialect"), so you match the shell automatically. |

**The golden rule:** never set your own position, never draw your own window
chrome, never assume your size. You declare *how big your content naturally
wants to be*; Ryoku does the rest. Break this rule and your plugin will look
bolted-on instead of native.

---

## Files you ship

```
<your-plugin-id>/
  manifest.json        what your plugin is + its suggested defaults   (required)
  service/Main.qml     persistent logic and state, no UI               (required)
  content/Widget.qml   ONE view of your widget                          (required)
  bin/                 scripts or binaries your plugin runs            (optional)
  README.md            what it is + a preview.gif                      (recommended)
```

- **Installed** to `~/.local/share/ryoku/plugins/<id>/`.
- **For local dev**, point `RYOSTORE_PLUGINS_DIR` at a folder of plugins (colon-
  separated for several) and the dev shell discovers them live.

---

## The three hosts a user can choose

When a user enables your plugin they pick **one** of these in Ryoku Settings →
Plugins. Your *same* `content/Widget.qml` is used for all three - Ryoku just
renders it differently.

### 1. Desktop widget - a tile on the wallpaper

It sits on the wallpaper next to the clock and weather, and behaves **exactly
like them**, because Ryoku gives it the identical machinery:

- **Left-drag** to move it anywhere (snaps to a grid).
- **Right-click** it for its menu (Lock to freeze it, Hide to turn it off).
- **Drag the bottom-right corner** to resize (scales 50%–250%, live).
- It's drawn on a **card** (rounded, translucent, soft shadow) matching the
  clock/weather.

**You do nothing to get any of that.** You just write the content; Ryoku wraps it
in the draggable, resizable, right-clickable card. The only thing you owe it: a
content root that reports its natural size (see "Sizing" below), so the card can
size itself around you.

### 2. Frame popout - grows out of a screen edge, or floats at the centre

It melts out of the frame border on hover (like the volume mixer and power
menu), fused into the same blob. The user picks the **edge** (top / right /
bottom / left) and where along it the popout sits (`start`, `center`, `end`), by
dragging the popout chip around a mock of their screen in Ryoku Settings.

Dropped in the **middle** of that screen instead, the popout becomes a centred
surface: it floats in the middle of the display, all four corners rounded, with
no hover edge. A centred popout opens only when it is asked for, by
`ryoku-shell plugin <id>` or a click, which makes it the placement
for a modal view. It shares the middle of the screen with quick settings
(`Super+Escape`) and the stash (`Super+S`), but the shell only ever shows one
surface at a time, so they take turns rather than overlap.

- Ryoku handles the **hover trigger, the open/close animation, and the fuse into
  the frame**.
- **Dimensions & Sizing**:
  - By default, edge popouts use a compact `360px` width budget and grow vertically
    to fit your content's `implicitHeight`.
  - For rich or modal views (calculators, email, system monitors), you can specify
    a custom width and height:
    - **In manifest defaults**: declare `"width"` and/or `"height"` under `defaults.framePopout`.
    - **In plugin settings**: declare `"width"` and/or `"height"` controls in `metadata.settings`.
      Ryoku Settings will expose them directly to users, and the popout will resize live.
    - **Implicit fallback**: if no fixed setting is set, Ryoku respects your root
      `implicitWidth` (falling back to standard 680px for centered popouts).
- **Dismissal & Lifecycle**:
  - **Edge hover popouts** auto-retract after the pointer leaves (`autoDismiss: true`).
  - **Centered modal popouts** stay open until explicitly closed via `Escape`, clicking
    outside, or toggling the shortcut (`autoDismiss: false`).
  - To override this behavior on any popout, set `"autoDismiss": true` or `false`
    in `defaults.framePopout`.

`ryoku-shell plugin <id>` toggles your frame popout open (bind it to a key).

### 3. Bar glyph - rides the bar

It rides the top bar as its own module, in a section you choose (left, centre or
right) and at the position in that lane you drag it to. Your content is rendered
at `glyph` density on the bar's axis, so it must stay small: report a mark-sized
`implicitWidth`/`implicitHeight` (roughly the bar's inner height) and Ryoku
centres it and reserves exactly that much room; in islands form it gets its own
island.

- Ryoku handles the **island pill, spacing, and the bar's width arithmetic**.
- A bar glyph **is a layout entry**, the same kind of entry as a built-in widget:
  it lives in `shell.json` `qsbar.layout`, so a user drags it between lanes in QS
  Bar Settings' Layout route or moves it from a terminal with `ryoku-shell bar
  move <id> --section left|center|right`. It shows once it is enabled with
  `host: topbarGlyph` in `plugins.json`.

Declare `topbarGlyph` in `hosts`, and include `"glyph"` in
`capabilities.densities` - a plugin that only draws a card is a poor bar glyph.

What the bar host sets on your `content` (read them, never assign them):

| Property | Value on the bar |
| --- | --- |
| `density` | `"glyph"` |
| `widthBudget` | `220` (logical px; cap any text you draw to it) |
| `active` | `true` while mounted |
| `pluginApi` | your handle: `mainInstance`, `pluginSettings`, `pluginDir` |

The host reads your `implicitWidth`/`implicitHeight` and centres the view in a
slot about 32 px tall. Draw the mark with the shell's own icon font: `Text {
font.family: "Material Symbols Rounded"; text: "vpn_lock" }` (the font ships
with the desktop, ligature names are the symbol names). Hover text is yours to
add; `QtQuick.Controls`' `ToolTip` resolves inside a plugin and matches the
shell when styled from the kit `Theme`. On the bar your settings are rendered
by QS Bar Settings, which draws these `metadata.settings` types: `toggle`,
`choice` (a segment bar up to four plain options, chips beyond), `multi`, `int`
(a stepper with `min`/`max`) and `text`; `slider` and `image` are desktop-widget
menu types and do not render there, so a bar widget picks from the first set.

Install a bar plugin straight from git, or from a folder on this desktop, with
`ryoku plugin add <git-url|dir> --bar`: it validates the manifest, installs the
files through Ryostore's transaction (a receipt and a content-hashed copy under
`~/.local/share/ryoku/plugins/<id>`), enables it on the bar, and the shell
picks it up on the `plugins.json` change, no restart. It lists under QS Bar
Settings > Community (any plugin whose manifest is not `"official": true`)
with the community warning, its author, its switch and settings, and
EXPORT / SHARE TO RYOSTORE / REMOVE. `ryoku plugin list|remove|validate`
manage it from a terminal; see "Share it" below for export and share.

> Island and window hosts are planned but not built yet. Declare only
> `framePopout`, `desktopWidget`, or `topbarGlyph` in your manifest today.

---

## Writing `content/Widget.qml`

This is your view. Ryoku sets a few properties on its root **for you to read** -
**never assign them**:

| Property Ryoku sets | What it means | What you do with it |
|---|---|---|
| `pluginApi` | handle to your service + settings + folder | read `pluginApi.mainInstance` for your live `service/Main.qml` |
| `density` | `"glyph"`, `"compact"`, or `"full"` | lay out smaller/larger for the space you're given |
| `widthBudget` | the width you have to lay out within | size your content to this width, not to the screen |
| `active` | true while your widget is open/visible | start/stop work (e.g. don't poll when hidden) |
| `screen` | the monitor this copy is on | usually ignore it |

### Sizing - the one thing you MUST get right

**Declare your content's natural size; never hardcode geometry.** Ryoku reads
your root's `implicitWidth` / `implicitHeight` to size the card or grow the
popout. If you don't report a size, your widget collapses to nothing.

```qml
import QtQuick
import Ryoku.PluginKit          // the deck kit: Theme, Motion, Card, etc.

Item {
    id: root

    // Ryoku sets these; you only read them.
    property var pluginApi
    property string density: "full"
    property real widthBudget: 0
    property bool active: false

    readonly property var service: pluginApi ? pluginApi.mainInstance : null

    // Pick ONE content width from the budget, and lay everything out from it.
    readonly property real contentW: widthBudget > 0 ? widthBudget : 360

    // Report your natural size so the host can size its surface around you.
    implicitWidth: contentW
    implicitHeight: column.implicitHeight

    Column {
        id: column
        width: root.contentW          // bind children to contentW, NOT parent.width
        spacing: 12
        // ... your eyebrow, search, list, grid ...
    }
}
```

Rules that keep it native:
- **Bind children to your own `contentW`**, never to `parent.width` through
  nested layouts (that leaves width-derived heights at zero and your widget
  collapses).
- **Reflow with `Column` / `Row` / `Grid` / `Flow`** so you adapt to the width
  Ryoku gives you. Don't position with absolute `x`/`y`.
- **Three densities**: `glyph` = an icon (+ optional badge); `compact` = a small
  summary; `full` = the rich panel. Lay out for whichever `density` you're handed.

---

## Styling - use the kit, match the shell

Your runtime content imports the **deck kit** so it looks like the rest of the
shell automatically:

```qml
import Ryoku.PluginKit            // Theme, Motion, GlyphIcon, MicroLabel,
                                  // SearchField, Card, CornerTicks, WaveMeter
```

Use `Theme` colors and `Motion` curves - mono eyebrows, hairline dividers, the
vermillion accent, the project's morph timing. **Don't hardcode colors**; read
them from `Theme`. The vermillion accent is the one accent - use it sparingly.

Your **settings are not QML** - you declare them as a `metadata.settings` schema
in the manifest (below) and Ryoku renders native controls for them, both in
Ryoku Settings and in the desktop widget's right-click menu.

---

## `service/Main.qml` - your logic

A non-visual `QtObject`/`Item` that holds state and does the work (HTTP, running
your `bin/` scripts, parsing results). Ryoku keeps one instance alive and hands
it to your content as `pluginApi.mainInstance`.
Declare user options as a `metadata.settings` schema in your manifest (below).
Ryoku renders the controls, seeds your defaults when the plugin is enabled, and
persists changes to `plugins.json`; read the live values from
`pluginApi.pluginSettings`. Read every key behind its default anyway: a plugin
enabled before a setting existed, or a settings block a user trimmed, has no
value for it, and `undefined` must not become your poll interval.

---

## `manifest.json`

```json
{
  "id": "your-plugin",
  "name": "Your Plugin",
  "version": "1.0.0",
  "author": "You <you@example.com>",
  "description": "One sentence describing it.",
  "license": "MIT",
  "official": false,
  "entryPoints": {
    "main": "service/Main.qml",
    "content": "content/Widget.qml"
  },
  "files": ["content/Helper.qml", "assets/example.jpg"],
  "capabilities": { "densities": ["compact", "full"] },
  "hosts": ["framePopout", "desktopWidget"],
  "defaults": {
    "host": "framePopout",
    "framePopout": {
      "edge": "center",
      "align": "center",
      "width": 680,
      "height": 520,
      "autoDismiss": false
    },
    "icon": "image",
    "label": "Your Plugin"
  },
  "commands": ["bin/your-tool"],
  "dependencies": { "commands": ["curl", "jq"] },
  "metadata": {
    "settings": [
      { "key": "imagePath", "type": "image",  "label": "Image", "group": "Photo", "default": "" },
      { "key": "style", "type": "choice", "label": "Style", "group": "Photo", "default": "rounded",
        "options": [ { "value": "rounded", "label": "Rounded" }, { "value": "polaroid", "label": "Polaroid" } ] },
      { "key": "shadowEnabled", "type": "toggle", "label": "Drop shadow", "group": "Shadow", "default": true },
      { "key": "shadowBlur", "type": "slider", "label": "Blur", "group": "Shadow", "default": 0.5, "min": 0, "max": 1, "step": 0.01 }
    ]
  }
}
```

- `hosts` - declare **only the hosts you actually support and have tested**
  (today: `framePopout`, `desktopWidget`, `topbarGlyph`). Don't list hosts that
  don't work.
- `defaults` - *suggestions*. The user's choices in Settings always win. For
  `framePopout`, `align` is `start`, `center` or `end`, and `edge: "center"`
  asks for the centred (modal) surface, which opens only on request. A bar
  widget's defaults are just `{ "host": "topbarGlyph", "icon": "...", "label":
  "..." }`, plus an optional `"bar": { "section": "left|center|right" }` for
  the lane it first lands in (the end of the right lane when absent; the
  Layout route moves it from there, and that choice is kept). `icon` is a
  Material Symbols Rounded ligature name (`vpn_lock`, `extension`, ...), the
  mark menus and pickers show for the plugin.
- `official` - leave `false`. Only first-party Ryoku plugins set `true`; every
  other plugin lists under QS Bar Settings > Community and carries the store's
  community warning.
- **A plugin never gets a keybind of its own.** Ryoku has no plugins-menu leader
  and reads no `key` field from your manifest; do not ship one, and do not tell
  users a chord opens your plugin. A frame popout opens on hover at its edge, or
  through `ryoku-shell plugin <id>`, which the user can bind to whatever chord
  they like in Settings → Keybinds. The shipped bind table stays Ryoku's.
- `files` - any extra files the plugin ships beyond its entry points and
  `commands` (helper QML a view imports, images, data). Install fetches the entry
  points, `commands`, `README.md`, and everything listed here; a file you forget
  to list is simply missing on install, so the plugin can fail to render.

---

## Install, enable, place

- **Install**: `ryoku plugin add <git-url|dir> [--bar]` (validated, then
  installed through Ryostore's transaction so the shell's `discover.sh` loads
  it), or install it from Ryostore itself. Never hand-copy into
  `~/.local/share/ryoku/plugins/`: a folder without a receipt is not loaded.
- **Enable & place**: Ryoku Settings → Plugins. The user toggles it on, picks a
  host, and (for a frame popout) the edge. Placement saves to
  `~/.config/ryoku/plugins.json`; the shell watches that file and retunes live -
  no restart.
- **Desktop widgets** are then moved/resized/hidden directly on the wallpaper
  (drag, corner-resize, right-click) - not from Settings.

---

## Share it

A widget written on one desktop (by hand, or by asking Rashin) reaches every
other one through Ryostore. Two commands do the packaging, so the catalogue's
per-file hashes are never typed by hand:

```bash
ryoku plugin export vpn      # ~/Documents/ryoku-plugins/vpn/: the files,
                             # product-manifest.json, registry-entry.json, a git repo
ryoku plugin share vpn       # exports if needed, then opens the Ryostore pull
                             # request (gh logged in) or the submission form
```

`export` copies the installed plugin out, writes `product-manifest.json` (the
sha256/size/mode of every file, docs and preview media marked `install: false`,
executables `0755`) and `registry-entry.json` (a complete `plugins/registry.json`
row: `official: false`, `hosts` from the manifest, the `bar-widget` or
`desktop-widget` tag added), and puts the folder under git. `share` lays that
into a fork of `neur0map/ryostore` as `plugins/<id>/`, upserts the registry
entry, pushes `plugin/<id>` and opens the pull request with the catalogue's
checklist; without `gh` it opens the submission form prefilled and tells you to
push the folder somewhere public first. A real `assets/preview-widget.png` is
required either way. Both actions are also buttons on the plugin's row under QS
Bar Settings > Community.

Ryostore lists community plugins under a warning: the review is for listing, not
a security audit, and a plugin runs unsandboxed in the user's shell. The store's
Plugins tab parts BAR widgets (manifest `hosts` includes `topbarGlyph`) from
DESKTOP ones; tag yours `bar-widget` or `desktop-widget` to match.

---

## Gotcha: images in grid/list delegates

An `Image` inside an inline QML `component` used as a delegate **loads but never
paints** in Quickshell (a scene-graph quirk). Put image/thumbnail delegates
inline - a plain `Rectangle { Image {} }` - or in their own `.qml` file, **not**
inside an inline `component { ... }`. See `wallhaven`'s grid for the working
pattern.

