package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
)

// config mirrors the Rust daemon's ~/.config/ryoku/ryogami.json: only the keys
// the Go daemon consumes are modeled; unknown keys pass through untouched on
// the persist path because the file is rewritten by targeted patches only.
type config struct {
	Matugen struct {
		Mode       string `json:"mode"`
		SchemeType string `json:"schemeType"`
		ColorIndex uint32 `json:"colorIndex"`
	} `json:"matugen"`
	ResourceTier     string `json:"resource_tier"`
	RestoreOnStartup *bool  `json:"restoreOnStartup"`
	Features         struct {
		Wallpapers *bool `json:"wallpapers"`
		Matugen    *bool `json:"matugen"`
	} `json:"features"`
	Paths struct {
		Wallpaper      string `json:"wallpaper"`
		VideoWallpaper string `json:"videoWallpaper"`
		Cache          string `json:"cache"`
	} `json:"paths"`
}

func home() string {
	h, err := os.UserHomeDir()
	if err != nil {
		return "/"
	}
	return h
}

func ryokuConfigDir() string {
	if d := os.Getenv("XDG_CONFIG_HOME"); d != "" {
		return filepath.Join(d, "ryoku")
	}
	return filepath.Join(home(), ".config", "ryoku")
}

func configPath() string { return filepath.Join(ryokuConfigDir(), "ryogami.json") }

func loadConfig() config {
	var c config
	loadJSON(configPath(), &c)
	if c.Matugen.Mode == "" {
		c.Matugen.Mode = "dark"
	}
	if c.Matugen.SchemeType == "" {
		c.Matugen.SchemeType = "scheme-tonal-spot"
	}
	if c.ResourceTier == "" {
		c.ResourceTier = "medium"
	}
	return c
}

func resolvePath(p string) string {
	if p == "" {
		return ""
	}
	if strings.HasPrefix(p, "~/") {
		return filepath.Join(home(), p[2:])
	}
	return p
}

func (c config) wallpaperDir() string {
	if p := resolvePath(c.Paths.Wallpaper); p != "" {
		return p
	}
	return filepath.Join(home(), "Pictures", "Wallpapers")
}

func (c config) videoDir() string {
	if p := resolvePath(c.Paths.VideoWallpaper); p != "" {
		return p
	}
	return c.wallpaperDir()
}

func (c config) cacheDir() string {
	if p := resolvePath(c.Paths.Cache); p != "" {
		return p
	}
	if d := os.Getenv("XDG_CACHE_HOME"); d != "" {
		return filepath.Join(d, "ryogami")
	}
	return filepath.Join(home(), ".cache", "ryogami")
}

func (c config) wallpapersEnabled() bool {
	return c.Features.Wallpapers == nil || *c.Features.Wallpapers
}

func (c config) matugenEnabled() bool {
	return c.Features.Matugen == nil || *c.Features.Matugen
}

func (c config) restoreEnabled() bool {
	return c.RestoreOnStartup == nil || *c.RestoreOnStartup
}

// persistResourceTier patches only the resource_tier key so hand-edited keys in
// ryogami.json survive (the file is user-facing config, not daemon state).
func persistResourceTier(tier string) {
	raw := map[string]json.RawMessage{}
	loadJSON(configPath(), &raw)
	b, err := json.Marshal(tier)
	if err != nil {
		return
	}
	raw["resource_tier"] = b
	_ = os.MkdirAll(ryokuConfigDir(), 0o755)
	out, err := json.MarshalIndent(raw, "", "  ")
	if err != nil {
		return
	}
	saveRaw(configPath(), out)
}

func saveRaw(path string, b []byte) {
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o644); err != nil {
		return
	}
	_ = os.Rename(tmp, path)
}

// contentFit reads the shell's wallpaper fill mode from ryoku's shell.json,
// mirroring the Rust daemon: the shell owns that preference, the daemon only
// echoes it into published frames.
func contentFit() string {
	const def = "Cover"
	var shell struct {
		Wallpaper struct {
			Fit string `json:"content_fit"`
		} `json:"wallpaper"`
	}
	loadJSON(filepath.Join(ryokuConfigDir(), "shell.json"), &shell)
	switch shell.Wallpaper.Fit {
	case "Contain", "Cover", "Fill", "ScaleDown":
		return shell.Wallpaper.Fit
	}
	return def
}
