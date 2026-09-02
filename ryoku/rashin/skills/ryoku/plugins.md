# Shell plugins

A plugin is a small widget a user drops into the desktop. The contributor writes
the logic and one view; Ryoku owns how it looks, moves, sizes, and where it
lives, so a plugin always reads as a native part of the shell. Install plugins
from Ryostore (the curated front) or from any git repo with `ryoku plugin`.
Ryoku never runs a plugin's code to install it.

## What a plugin ships

```
<plugin-id>/
  manifest.json        what it is, its hosts, its defaults, and its settings schema
  service/Main.qml     persistent logic and state, no UI
  content/Widget.qml   one view, rendered at the host's density
  bin/                 optional scripts or binaries
  README.md            recommended, with a preview
```

Installed plugins live at `~/.local/share/ryoku/plugins/<id>/`. The manifest's
`hosts` names where it can sit; today the three are:

- `desktopWidget`: a draggable, resizable tile on the wallpaper.
- `framePopout`: a surface that grows from a screen edge on hover, or floats
  centred as a modal.
- `topbarGlyph`: a mark on the QS Bar, immediately left of the status cluster.
  A plugin that declares `topbarGlyph` appears in the bar's add-widget picker and
  moves like a built-in.

A plugin declares its user options as a `metadata.settings` schema in the
manifest; Ryoku renders native controls, seeds the defaults on install, and
persists changes to `~/.config/ryoku/plugins.json`. See `docs/plugins.md` in the
Ryoku source for the full authoring contract.

## The plugin CLI

`ryoku plugin` manages installed plugins. `add` clones to a staging directory,
validates the manifest, then moves it into place; it never executes anything
from the plugin.

```
ryoku plugin add <git-url|dir> [--bar] [--yes]   copy or clone, validate, install; --bar puts it on the bar
ryoku plugin list [--json]                       installed plugins and their enable/host state
ryoku plugin remove <id>                         uninstall (and drop its placement)
ryoku plugin validate <dir>                      check a manifest without installing
ryoku plugin export <id> [--to <dir>]            a Ryostore-shaped folder: files, product-manifest.json,
                                                 registry-entry.json, under git (default ~/Documents/ryoku-plugins/<id>)
ryoku plugin share <id>                          export, then open the Ryostore pull request (gh) or the form
```

A widget you write for the user goes in its own folder (say `~/ryoku-plugins/<id>/`
or a temp dir), then `ryoku plugin validate <dir>` and `ryoku plugin add <dir> --bar
--yes`; git is not needed for a local folder. It then shows on the bar and under QS
Bar Settings > Community, where the user can EXPORT or SHARE it. Do not run
`share` unless the user asks to publish: it opens a public pull request.

`--bar` enables the plugin and places it on the bar (host `topbarGlyph`) through
the shell's own placement path. `list --json` returns
`[{"id","name","version","hosts","dir","enabled","host"}]`.

Validation rejects an install when the manifest is malformed: a missing id, name,
or version; entry points that do not exist or are not relative (no `..`);
`hosts` outside `framePopout | desktopWidget | topbarGlyph`; a symlink anywhere
inside; an id that collides with a built-in widget id (those are reserved); or an
id already installed.

```bash
ryoku plugin add https://github.com/someone/ryoku-weather --bar
ryoku plugin add ~/my-widget --bar --yes   # a folder on this desktop
ryoku plugin list --json
ryoku plugin validate ./my-plugin
ryoku plugin remove ryoku-weather
ryoku plugin export my-widget              # a Ryostore-shaped folder + git repo
ryoku plugin share my-widget               # the Ryostore pull request (asked for)
```

## Ryostore

Ryostore is the curated distribution front for plugins, bar styles, and rices;
its Plugins tab parts BAR widgets (manifest `hosts` includes `topbarGlyph`) from
DESKTOP ones, and lists community plugins (not `"official": true`) under a
warning that Ryoku does not review or maintain them. Git and a local folder
remain the open door for anything not in the store. Either way the same
placement rules apply: enable it and pick its host in Ryoku Settings, or with
`ryoku plugin add --bar` for the bar. `ryoku plugin share <id>` is how a widget
made here gets listed there.
