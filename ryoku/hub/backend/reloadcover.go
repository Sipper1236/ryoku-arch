package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

const reloadCoverMaxBytes int64 = 64 << 20

var reloadCoverManagedName = regexp.MustCompile(`^[0-9a-f]{64}\.(bmp|gif|jpe?g|mkv|mov|mp4|png|svg|webm|webp)$`)

type reloadCoverAsset struct {
	Path  string `json:"path"`
	Name  string `json:"name"`
	Kind  string `json:"kind"`
	Bytes int64  `json:"bytes"`
}

func emptyReloadCoverAsset() reloadCoverAsset {
	return reloadCoverAsset{Kind: "default"}
}

func reloadCoverDir() string {
	return filepath.Join(dataHome(), "ryoku", "reload-cover")
}

func reloadCoverBrandPath() string {
	return filepath.Join(configHome(), "ryoku", "brand.json")
}

func reloadCoverKind(path string) (string, error) {
	ext := strings.ToLower(filepath.Ext(path))
	if isVideo(path) {
		return "video", nil
	}
	switch ext {
	case ".gif":
		return "animated", nil
	case ".bmp", ".jpeg", ".jpg", ".png", ".svg", ".webp":
		return "image", nil
	default:
		return "", fmt.Errorf("unsupported reload-cover file type %q", ext)
	}
}

func resolveReloadCoverSource(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", fmt.Errorf("reload-cover import needs a local file")
	}
	if strings.HasPrefix(raw, "file://") {
		parsed, err := url.Parse(raw)
		if err != nil {
			return "", fmt.Errorf("invalid file URL: %w", err)
		}
		if parsed.Host != "" && parsed.Host != "localhost" {
			return "", fmt.Errorf("reload-cover import accepts local files only")
		}
		decoded, err := url.PathUnescape(parsed.EscapedPath())
		if err != nil {
			return "", fmt.Errorf("invalid file URL path: %w", err)
		}
		raw = filepath.FromSlash(decoded)
	} else if strings.Contains(raw, "://") {
		return "", fmt.Errorf("reload-cover import accepts local files only")
	}
	if raw == "~" || strings.HasPrefix(raw, "~/") {
		raw = filepath.Join(os.Getenv("HOME"), strings.TrimPrefix(raw, "~/"))
	}
	absolute, err := filepath.Abs(raw)
	if err != nil {
		return "", err
	}
	return filepath.Clean(absolute), nil
}

func managedReloadCoverPath(path string) bool {
	if path == "" {
		return false
	}
	clean := filepath.Clean(path)
	return filepath.Dir(clean) == filepath.Clean(reloadCoverDir()) && reloadCoverManagedName.MatchString(filepath.Base(clean))
}

func committedReloadCoverPath() string {
	body, err := os.ReadFile(reloadCoverBrandPath())
	if err != nil {
		return ""
	}
	var doc struct {
		ReloadCover reloadCoverAsset `json:"reloadCover"`
	}
	if json.Unmarshal(body, &doc) != nil || !managedReloadCoverPath(doc.ReloadCover.Path) {
		return ""
	}
	return filepath.Clean(doc.ReloadCover.Path)
}

func pruneReloadCoverExcept(kept map[string]bool) error {
	dir := filepath.Clean(reloadCoverDir())
	for path := range kept {
		if !managedReloadCoverPath(path) {
			return fmt.Errorf("refusing reload-cover prune outside %s", dir)
		}
	}
	entries, err := os.ReadDir(dir)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return err
	}
	for _, entry := range entries {
		name := entry.Name()
		owned := reloadCoverManagedName.MatchString(name) || strings.HasPrefix(name, ".import-")
		if entry.IsDir() || !owned {
			continue
		}
		path := filepath.Join(dir, name)
		if kept[path] {
			continue
		}
		if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
			return err
		}
	}
	return nil
}

func pruneReloadCover(keep string) error {
	kept := map[string]bool{}
	if keep != "" {
		kept[filepath.Clean(keep)] = true
	}
	return pruneReloadCoverExcept(kept)
}

func importReloadCover(raw string) (reloadCoverAsset, error) {
	source, err := resolveReloadCoverSource(raw)
	if err != nil {
		return reloadCoverAsset{}, err
	}
	info, err := os.Stat(source)
	if err != nil {
		return reloadCoverAsset{}, err
	}
	if !info.Mode().IsRegular() {
		return reloadCoverAsset{}, fmt.Errorf("reload-cover import needs a regular file")
	}
	input, err := os.Open(source)
	if err != nil {
		return reloadCoverAsset{}, err
	}
	defer input.Close()
	if info.Size() > reloadCoverMaxBytes {
		return reloadCoverAsset{}, fmt.Errorf("reload-cover file exceeds 64 MiB (20 MB)")
	}
	kind, err := reloadCoverKind(source)
	if err != nil {
		return reloadCoverAsset{}, err
	}
	if err := os.MkdirAll(reloadCoverDir(), 0o755); err != nil {
		return reloadCoverAsset{}, err
	}
	temp, err := os.CreateTemp(reloadCoverDir(), ".import-")
	if err != nil {
		return reloadCoverAsset{}, err
	}
	tempPath := temp.Name()
	defer func() {
		if tempPath != "" {
			os.Remove(tempPath)
		}
	}()
	hash := sha256.New()
	bytes, err := io.Copy(io.MultiWriter(temp, hash), io.LimitReader(input, reloadCoverMaxBytes+1))
	if err != nil {
		temp.Close()
		return reloadCoverAsset{}, err
	}
	if bytes > reloadCoverMaxBytes {
		temp.Close()
		return reloadCoverAsset{}, fmt.Errorf("reload-cover file exceeds 64 MiB (20 MB)")
	}
	if err := temp.Sync(); err != nil {
		temp.Close()
		return reloadCoverAsset{}, err
	}
	if err := temp.Chmod(0o644); err != nil {
		temp.Close()
		return reloadCoverAsset{}, err
	}
	if err := temp.Close(); err != nil {
		return reloadCoverAsset{}, err
	}
	destination := filepath.Join(reloadCoverDir(), hex.EncodeToString(hash.Sum(nil))+strings.ToLower(filepath.Ext(source)))
	if _, err := os.Stat(destination); err != nil {
		if !os.IsNotExist(err) {
			return reloadCoverAsset{}, err
		}
		if err := os.Rename(tempPath, destination); err != nil {
			return reloadCoverAsset{}, err
		}
	}
	tempPath = ""
	kept := map[string]bool{destination: true}
	if committed := committedReloadCoverPath(); committed != "" {
		kept[committed] = true
	}
	if err := pruneReloadCoverExcept(kept); err != nil {
		return reloadCoverAsset{}, err
	}
	return reloadCoverAsset{
		Path:  destination,
		Name:  filepath.Base(source),
		Kind:  kind,
		Bytes: bytes,
	}, nil
}

func runReloadCover(args []string) error {
	if len(args) == 0 {
		return fmt.Errorf("reload-cover needs import|prune")
	}
	switch args[0] {
	case "import":
		if len(args) != 2 {
			return fmt.Errorf("reload-cover import needs one local path")
		}
		asset, err := importReloadCover(args[1])
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(asset)
	case "prune":
		if len(args) > 2 {
			return fmt.Errorf("reload-cover prune accepts at most one managed path")
		}
		keep := ""
		if len(args) == 2 {
			keep = args[1]
		}
		return pruneReloadCover(keep)
	default:
		return fmt.Errorf("reload-cover needs import|prune")
	}
}
