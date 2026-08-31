package main

import "fmt"

const daemonVersion = "0.2.0"

// dispatchRequest routes one JSON-RPC request. The method set and response
// shapes are the wire contract the wall-ui picker parses; unimplemented
// subsystems (effects, optimize, video_convert, analysis, steam) answer with
// the standard unknown-method error, which the picker's default feature set
// never triggers.
func (d *daemon) dispatchRequest(req *request) response {
	p := req.params()
	switch req.Method {
	case "status":
		return ok(req.ID, map[string]interface{}{
			"version":           daemonVersion,
			"current_wallpaper": nullable(d.currentName()),
		})

	case "state.get":
		if v, okKey := d.store.stateGet(strParam(p, "key", "")); okKey {
			return ok(req.ID, map[string]interface{}{"value": v})
		}
		return ok(req.ID, map[string]interface{}{"value": nil})

	case "state.set":
		key := strParam(p, "key", "")
		if key == "" {
			return errResp(req.ID, 1, "missing 'key' parameter")
		}
		if v, has := p["value"].(string); has {
			d.store.stateSet(key, &v)
		} else {
			d.store.stateSet(key, nil)
		}
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.list":
		rows := d.store.list(boolParam(p, "favourites", false))
		return ok(req.ID, map[string]interface{}{"count": len(rows), "wallpapers": rows})

	case "wall.apply":
		wpType := strParam(p, "type", "static")
		if wpType != "static" && wpType != "video" {
			return errResp(req.ID, 1, fmt.Sprintf("unsupported type: %s", wpType))
		}
		if err := d.applyWallpaper(wpType, strParam(p, "path", ""), strsParam(p, "outputs")); err != nil {
			return errResp(req.ID, 4, err.Error())
		}
		return ok(req.ID, map[string]interface{}{"applied": d.currentName()})

	case "wall.restore":
		d.restoreOutputs()
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.set_favourite":
		key := strParam(p, "key", "")
		fav := 0
		if boolParam(p, "favourite", false) {
			fav = 1
		}
		if !d.store.mutate(key, func(e *Entry) { e.Favourite = fav }) {
			return errResp(req.ID, 2, fmt.Sprintf("unknown wallpaper: %s", key))
		}
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.update_metadata":
		key := strParam(p, "key", "")
		d.store.mutate(key, func(e *Entry) {
			if v := intParam(p, "filesize", 0); v > 0 {
				e.Filesize = v
			}
			if v := intParam(p, "width", 0); v > 0 {
				e.Width = int(v)
			}
			if v := intParam(p, "height", 0); v > 0 {
				e.Height = int(v)
			}
		})
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.update_analysis":
		key := strParam(p, "key", "")
		found := d.store.mutate(key, func(e *Entry) {
			e.Tags = optStr(p, "tags")
			e.Colors = optStr(p, "colors")
			e.AnalyzedBy = optStr(p, "analyzed_by")
			if v, has := p["hue"].(float64); has {
				e.Hue = int(v)
			}
			if v, has := p["sat"].(float64); has {
				e.Sat = int(v)
			}
		})
		if !found {
			return errResp(req.ID, 2, fmt.Sprintf("unknown wallpaper: %s", key))
		}
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.delete":
		if err := d.deleteWallpaper(strParam(p, "key", "")); err != nil {
			return errResp(req.ID, 2, err.Error())
		}
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.import":
		if err := d.importWallpaper(strParam(p, "path", "")); err != nil {
			return errResp(req.ID, 2, err.Error())
		}
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.outputs":
		return ok(req.ID, map[string]interface{}{"outputs": d.outputsState()})

	case "wall.set_audio":
		var mute *bool
		if v, has := p["mute"].(bool); has {
			mute = &v
		}
		d.setAudio(mute, strsParam(p, "outputs"))
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.cache_rebuild":
		go d.rescan(true)
		return ok(req.ID, map[string]interface{}{"started": true})

	case "wall.recompute_colors":
		go d.rescan(true)
		return ok(req.ID, map[string]interface{}{"started": true})

	case "wall.cache_status":
		return ok(req.ID, map[string]interface{}{"ready": true, "count": len(d.store.list(false))})

	case "wall.toggle":
		visible := d.ui.toggle()
		d.broadcast("ryogami.wall.toggle", map[string]interface{}{"visible": visible})
		return ok(req.ID, map[string]interface{}{"toggled": true, "visible": visible})

	case "wall.show":
		d.ui.launch()
		d.broadcast("ryogami.wall.show", map[string]interface{}{})
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.hide":
		d.ui.kill()
		d.broadcast("ryogami.wall.hide", map[string]interface{}{})
		return ok(req.ID, map[string]interface{}{"ok": true})

	case "wall.random_start":
		interval := intParam(p, "interval", 300)
		types := strsParam(p, "types")
		favOnly := boolParam(p, "favourites_only", false)
		d.random.start(interval, types, favOnly, func() { d.randomPick(types, favOnly) })
		d.broadcast("ryogami.wall.random_started", map[string]interface{}{
			"interval": interval, "types": types, "favourites_only": favOnly,
		})
		return ok(req.ID, map[string]interface{}{"started": true})

	case "wall.random_stop":
		d.random.stop()
		d.broadcast("ryogami.wall.random_stopped", map[string]interface{}{})
		return ok(req.ID, map[string]interface{}{"stopped": true})

	case "wall.random_status":
		return ok(req.ID, d.random.status())

	default:
		return errResp(req.ID, -32601, fmt.Sprintf("unknown method: %s", req.Method))
	}
}

func optStr(p map[string]interface{}, key string) *string {
	if v, has := p[key].(string); has {
		return &v
	}
	return nil
}

func nullable(s string) interface{} {
	if s == "" {
		return nil
	}
	return s
}
