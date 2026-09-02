package doctor

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestReconcileZen(t *testing.T) {
	// No Zen install anywhere is a clean no-op, so an update never touches a
	// user who does not have Zen.
	if r := reconcileZenInto([]string{filepath.Join(t.TempDir(), "absent")}, false); r.status != recOK {
		t.Fatalf("absent Zen: status %v, want ok", r.status)
	}

	root := t.TempDir()
	dst := filepath.Join(root, "distribution", "policies.json")

	// Check-only reports the pending apply and writes nothing.
	if r := reconcileZenInto([]string{root}, true); r.status != recWouldFix {
		t.Fatalf("check-only: status %v, want todo", r.status)
	}
	if _, err := os.Stat(dst); !os.IsNotExist(err) {
		t.Fatal("check-only wrote the policy; it must only report")
	}

	// Apply writes the embedded policy verbatim.
	if r := reconcileZenInto([]string{root}, false); r.status != recFixed {
		t.Fatalf("apply: status %v, want fixed", r.status)
	}
	got, err := os.ReadFile(dst)
	if err != nil {
		t.Fatalf("policy not written: %v", err)
	}
	if !bytes.Equal(bytes.TrimSpace(got), bytes.TrimSpace(zenPolicies)) {
		t.Fatal("written policy does not match the embedded payload")
	}

	// The payload is valid JSON and ships the two chosen extensions.
	var doc struct {
		Policies struct {
			ExtensionSettings map[string]any `json:"ExtensionSettings"`
		} `json:"policies"`
	}
	if err := json.Unmarshal(got, &doc); err != nil {
		t.Fatalf("policy is not valid JSON: %v", err)
	}
	for _, id := range []string{"uBlock0@raymondhill.net", "jid1-MnnxcxisBPnSXQ@jetpack"} {
		if _, ok := doc.Policies.ExtensionSettings[id]; !ok {
			t.Fatalf("policy is missing shipped extension %s", id)
		}
	}

	// A second run changes nothing (idempotent).
	if r := reconcileZenInto([]string{root}, false); r.status != recOK {
		t.Fatalf("second apply: status %v, want ok (idempotent)", r.status)
	}

	// A drifted policy is reconverged.
	if err := os.WriteFile(dst, []byte("{}\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if r := reconcileZenInto([]string{root}, false); r.status != recFixed {
		t.Fatalf("stale rewrite: status %v, want fixed", r.status)
	}
}
