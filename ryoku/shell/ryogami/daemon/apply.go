package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// The apply path: publish the frame the shell QML renders, persist the
// per-output state for restore, bump the catalog's apply count, trigger the
// matugen palette, and broadcast the applied event the picker listens for.

// applyWallpaper handles static and video applies. Stills render on the
// in-shell surface (with a reveal transition); videos play through mpvpaper on
// the background layer, and the frame's live flag makes the shell painter
// yield to it.
func (d *daemon) applyWallpaper(wpType, path, mode string, outputs []string, mute map[string]bool, volume map[string]int) error {
	if path == "" {
		return fmt.Errorf("missing 'path' parameter")
	}
	if _, err := os.Stat(path); err != nil {
		return fmt.Errorf("wallpaper not readable: %v", err)
	}
	fit := contentFit()
	live := wpType == "video"
	// A reveal transition is an image operation; a video's frame swap is a cut.
	var tr interface{}
	if !live {
		if picked := d.transitionFor(mode); picked != nil {
			tr = picked
		}
	}
	if live {
		d.video.Play(outputs, path, mute, volume)
	} else if d.video.Playing() {
		d.video.Stop()
	}
	if len(outputs) == 0 || contains(outputs, "*") {
		d.surface.show(path, fit, tr, live)
	} else {
		for _, out := range outputs {
			d.surface.showOutput(out, path, fit, tr, live)
		}
	}
	name := filepath.Base(path)
	d.setCurrent(name)
	d.saveOutputs(outputs, wpType, path, mute)
	key := strings.TrimSuffix(name, filepath.Ext(name))
	d.store.mutate(keyFor(d.store, name, key), func(e *Entry) { e.ApplyCount++ })
	if !live {
		go d.runMatugen(path)
	}
	d.broadcast("ryogami.wall.applied", map[string]interface{}{
		"type": wpType, "name": name, "path": path, "we_id": "", "key": key,
	})
	return nil
}

// keyFor resolves the store key for an applied file: entries under subfolders
// carry the relative name, so a basename-derived key needs a fallback search.
func keyFor(s *store, name, key string) string {
	if _, okKey := s.get(key); okKey {
		return key
	}
	for _, e := range s.list(false) {
		if filepath.Base(e.Name) == name {
			return e.Key
		}
	}
	return key
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}

// saveOutputs persists {output: {type, path}} to cacheDir/outputs.json for the
// startup restore, mirroring the Rust daemon: a broadcast apply clears the map
// to a single "*" entry, a per-output apply removes "*".
func (d *daemon) saveOutputs(outputs []string, wpType, path string, mute map[string]bool) {
	cacheDir := d.config().cacheDir()
	state := map[string]map[string]interface{}{}
	loadJSON(filepath.Join(cacheDir, "outputs.json"), &state)
	keys := outputs
	if len(keys) == 0 || contains(keys, "*") {
		keys = []string{"*"}
		state = map[string]map[string]interface{}{}
	} else {
		delete(state, "*")
	}
	for _, k := range keys {
		state[k] = map[string]interface{}{"type": wpType, "path": path, "mute": mute[k]}
	}
	_ = os.MkdirAll(cacheDir, 0o755)
	saveJSON(filepath.Join(cacheDir, "outputs.json"), state)
}

// restoreOutputs republishes the persisted wallpaper on startup so the shell
// never sits on the empty retained frame after a daemon restart.
func (d *daemon) restoreOutputs() {
	cacheDir := d.config().cacheDir()
	state := map[string]map[string]interface{}{}
	loadJSON(filepath.Join(cacheDir, "outputs.json"), &state)
	fit := contentFit()
	restored := ""
	restore := func(out string, e map[string]interface{}) {
		p, _ := e["path"].(string)
		if p == "" || !fileExists(p) {
			return
		}
		live := e["type"] == "video"
		if live {
			mute := map[string]bool{out: e["mute"] == true}
			var outs []string
			if out != "*" {
				outs = []string{out}
			}
			d.video.Play(outs, p, mute, nil)
		}
		if out == "*" {
			d.surface.show(p, fit, nil, live)
		} else {
			d.surface.showOutput(out, p, fit, nil, live)
		}
		restored = filepath.Base(p)
	}
	if e, okAll := state["*"]; okAll {
		restore("*", e)
	} else {
		for out, e := range state {
			restore(out, e)
		}
	}
	if restored != "" {
		d.setCurrent(restored)
		fmt.Fprintf(os.Stderr, "ryogami: auto-restored wallpaper: %s\n", restored)
	}
}

func fileExists(p string) bool {
	st, err := os.Stat(p)
	return err == nil && st.Mode().IsRegular()
}

// runMatugen regenerates the palette from the applied wallpaper with the
// configured scheme, matching the Rust daemon's invocation; failures are the
// template's problem, never the apply's.
func (d *daemon) runMatugen(path string) {
	cfg := d.config()
	if !cfg.matugenEnabled() {
		return
	}
	matugenCfg := filepath.Join(home(), ".config", "matugen", "config.toml")
	if !fileExists(matugenCfg) {
		return
	}
	args := []string{
		"-c", matugenCfg,
		"image", path,
		"-t", cfg.Matugen.SchemeType,
		"-m", cfg.Matugen.Mode,
		"--source-color-index", fmt.Sprint(cfg.Matugen.ColorIndex),
	}
	cmd := exec.Command("matugen", args...)
	out, err := cmd.CombinedOutput()
	if err != nil {
		fmt.Fprintf(os.Stderr, "ryogami: matugen failed: %v: %s\n", err, strings.TrimSpace(string(out)))
	}
}

// outputsState answers wall.outputs from the persisted map, adding the mute
// flag the picker's monitor popup reads (audio routing itself is the shell's
// domain, so mute is echoed state, not a mixer control).
func (d *daemon) outputsState() map[string]interface{} {
	state := map[string]map[string]interface{}{}
	loadJSON(filepath.Join(d.config().cacheDir(), "outputs.json"), &state)
	out := map[string]interface{}{}
	for k, e := range state {
		entry := map[string]interface{}{"type": e["type"], "mute": e["mute"] == true}
		if p, okPath := e["path"].(string); okPath {
			entry["path"] = p
		}
		out[k] = entry
	}
	return out
}

func (d *daemon) setAudio(mute *bool, outputs []string) {
	cacheDir := d.config().cacheDir()
	state := map[string]map[string]interface{}{}
	loadJSON(filepath.Join(cacheDir, "outputs.json"), &state)
	for k, e := range state {
		if len(outputs) > 0 && !contains(outputs, k) {
			continue
		}
		if mute != nil {
			e["mute"] = *mute
			state[k] = e
		}
	}
	saveJSON(filepath.Join(cacheDir, "outputs.json"), state)
}

// deleteWallpaper removes the source file and the catalog row, then tells
// every client the file is gone.
func (d *daemon) deleteWallpaper(key string) error {
	e, okKey := d.store.remove(key)
	if !okKey {
		return fmt.Errorf("unknown wallpaper: %s", key)
	}
	src := e.VideoFile
	if src == "" {
		src = filepath.Join(d.config().wallpaperDir(), e.Name)
	}
	if err := os.Remove(src); err != nil && !os.IsNotExist(err) {
		return err
	}
	for _, t := range []string{e.Thumb, e.ThumbSm} {
		if t != "" {
			_ = os.Remove(t)
		}
	}
	d.broadcast("ryogami.wall.file_removed", map[string]interface{}{"name": e.Name, "type": e.Type})
	return nil
}

// importWallpaper copies a file into the wallpaper dir and rescans, so the new
// entry flows to clients through the cached event.
func (d *daemon) importWallpaper(src string) error {
	if !fileExists(src) {
		return fmt.Errorf("source not readable: %s", src)
	}
	dst := filepath.Join(d.config().wallpaperDir(), filepath.Base(src))
	b, err := os.ReadFile(src)
	if err != nil {
		return err
	}
	if err := os.WriteFile(dst, b, 0o644); err != nil {
		return err
	}
	now := time.Now()
	_ = os.Chtimes(dst, now, now)
	go d.rescan(true)
	return nil
}

// marshalable sanity check for events carrying Entry values.
var _ = json.Marshal
