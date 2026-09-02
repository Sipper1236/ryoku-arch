package doctor

import (
	"bytes"
	_ "embed"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"ryoku-cli/internal/sys"
)

// zenPolicies is the Ryoku Zen policy, shipped in the ryoku binary and written
// verbatim into a detected Zen install. It is a Firefox enterprise policies.json
// (Zen is a Firefox fork and honours it on Linux): the shipped extensions
// (uBlock Origin, Privacy Badger, both installed removable, not forced) and the
// Wayland / hardware-decode / privacy pref defaults, set as defaults the user
// can still override.
//
//go:embed zen_policies.json
var zenPolicies []byte

// zenInstallRoots lists the directories a Zen install may live under. The policy
// belongs in <root>/distribution/policies.json, where it applies to every
// profile without touching the user's own settings. The list covers the AUR
// package (zen-browser-bin), a plain /opt drop, a per-user tarball, and whatever
// dir a zen binary on PATH resolves into.
func zenInstallRoots() []string {
	roots := []string{
		"/usr/lib/zen-browser",
		"/usr/lib64/zen-browser",
		"/opt/zen-browser-bin",
		"/opt/zen",
	}
	if home := homeDir(); home != "" {
		roots = append(roots, filepath.Join(home, ".local", "opt", "zen"))
	}
	for _, name := range []string{"zen", "zen-browser", "zen-bin"} {
		if p, err := exec.LookPath(name); err == nil {
			if real, err := filepath.EvalSymlinks(p); err == nil {
				roots = append(roots, filepath.Dir(real))
			}
		}
	}
	return roots
}

// reconcileZen writes the Ryoku Zen policy into every Zen install it finds. It
// is a no-op when Zen is absent, so an update never installs Zen or touches the
// browser for a user who does not have it; Zen ships only on the ISO and the
// install script. When Zen is present the policy converges idempotently. It
// never sets the default browser and never edits a user profile, so a Zen user's
// own choices stand.
func reconcileZen(checkOnly bool) recResult {
	return reconcileZenInto(zenInstallRoots(), checkOnly)
}

func reconcileZenInto(roots []string, checkOnly bool) recResult {
	want := bytes.TrimSpace(zenPolicies)
	var present, pending, did []string
	seen := map[string]bool{}
	for _, root := range roots {
		if root == "" || seen[root] || !sys.Exists(root) {
			continue
		}
		seen[root] = true
		present = append(present, root)
		dst := filepath.Join(root, "distribution", "policies.json")
		if cur, err := os.ReadFile(dst); err == nil && bytes.Equal(bytes.TrimSpace(cur), want) {
			continue
		}
		if checkOnly {
			pending = append(pending, root)
			continue
		}
		if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
			return failRes("could not create the Zen distribution dir at %s: %v", root, err)
		}
		if err := os.WriteFile(dst, append(append([]byte{}, want...), '\n'), 0o644); err != nil {
			return failRes("could not write the Zen policy at %s: %v", dst, err)
		}
		did = append(did, root)
	}
	switch {
	case len(present) == 0:
		return okRes("Zen not installed")
	case checkOnly && len(pending) > 0:
		return wouldRes("apply the Ryoku Zen policy in: %s", strings.Join(pending, ", "))
	case len(did) > 0:
		return fixedRes("applied the Ryoku Zen policy in: %s", strings.Join(did, ", "))
	default:
		return okRes("Zen policy up to date")
	}
}
