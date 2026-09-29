package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestReadTLPAssignmentsAndOwnership(t *testing.T) {
	path := filepath.Join(t.TempDir(), "settings.conf")
	data := "# USB_AUTOSUSPEND=0\nUSB_AUTOSUSPEND=1\nPCIE_ASPM_ON_AC='powersave'\nOTHER=value\n"
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}
	values, err := readTLPAssignments(path)
	if err != nil {
		t.Fatal(err)
	}
	if values["USB_AUTOSUSPEND"] != "1" || values["PCIE_ASPM_ON_AC"] != "powersave" || len(values) != 2 {
		t.Fatalf("unexpected parsed values: %#v", values)
	}
	rows := tlpSettingRows(map[string]tlpConfigValue{
		"USB_AUTOSUSPEND": {Value: "0", Source: "/etc/tlp.conf"},
	})
	if rows[0].Editable || rows[0].Reason == "" || rows[0].Value != "0" {
		t.Fatalf("external setting should be visible and locked: %#v", rows[0])
	}
	if !rows[1].Editable || rows[1].Value != "default" {
		t.Fatalf("default setting should be editable: %#v", rows[1])
	}
}

func TestTLPSettingAllowlist(t *testing.T) {
	for _, c := range []struct {
		id, value string
		valid     bool
	}{
		{"USB_AUTOSUSPEND", "0", true},
		{"PCIE_ASPM_ON_BAT", "powersupersave", true},
		{"PCIE_ASPM_ON_BAT", "$(touch /tmp/pwned)", false},
		{"STOP_CHARGE_THRESH_BAT0", "80", false},
	} {
		if got := validTLPSetting(c.id, c.value); got != c.valid {
			t.Errorf("validTLPSetting(%q, %q) = %v, want %v", c.id, c.value, got, c.valid)
		}
	}
}
