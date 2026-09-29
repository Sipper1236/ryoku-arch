package main

import (
	"bufio"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"golang.org/x/sys/unix"
)

const ryokuTLPConfig = "/etc/tlp.d/99-ryoku.conf"
const tlpASPMPolicyPath = "/sys/module/pcie_aspm/parameters/policy"

type tlpSettingDef struct {
	ID      string   `json:"id"`
	Label   string   `json:"label"`
	Group   string   `json:"group"`
	Default string   `json:"default"`
	Options []string `json:"options"`
}

var tlpSettingDefs = []tlpSettingDef{
	{ID: "USB_AUTOSUSPEND", Label: "USB autosuspend", Group: "USB", Default: "1", Options: []string{"0", "1"}},
	{ID: "PCIE_ASPM_ON_AC", Label: "PCIe ASPM on AC", Group: "PCIe", Default: "default", Options: []string{"default", "performance", "powersave", "powersupersave"}},
	{ID: "PCIE_ASPM_ON_BAT", Label: "PCIe ASPM on battery", Group: "PCIe", Default: "default", Options: []string{"default", "performance", "powersave", "powersupersave"}},
}

type tlpConfigValue struct {
	Value  string
	Source string
}

type tlpSettingRow struct {
	ID       string   `json:"id"`
	Label    string   `json:"label"`
	Group    string   `json:"group"`
	Default  string   `json:"default"`
	Value    string   `json:"value"`
	Source   string   `json:"source"`
	Options  []string `json:"options"`
	Editable bool     `json:"editable"`
	Reason   string   `json:"reason,omitempty"`
}

func readTLPAssignments(path string) (map[string]string, error) {
	f, err := os.Open(path)
	if os.IsNotExist(err) {
		return map[string]string{}, nil
	}
	if err != nil {
		return nil, err
	}
	defer f.Close()
	values := make(map[string]string)
	scan := bufio.NewScanner(f)
	for scan.Scan() {
		line := strings.TrimSpace(scan.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		key, value, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key = strings.TrimSpace(key)
		value = strings.Trim(strings.TrimSpace(value), "\"'")
		for _, def := range tlpSettingDefs {
			if key == def.ID {
				values[key] = value
			}
		}
	}
	return values, scan.Err()
}

func tlpConfigSources() (map[string]tlpConfigValue, error) {
	paths, err := filepath.Glob("/etc/tlp.d/*.conf")
	if err != nil {
		return nil, err
	}
	sort.Strings(paths)
	paths = append(paths, "/etc/tlp.conf")
	values := make(map[string]tlpConfigValue)
	for _, path := range paths {
		assignments, err := readTLPAssignments(path)
		if err != nil {
			return nil, err
		}
		for key, value := range assignments {
			values[key] = tlpConfigValue{Value: value, Source: path}
		}
	}
	return values, nil
}

func tlpSettingRows(values map[string]tlpConfigValue) []tlpSettingRow {
	rows := make([]tlpSettingRow, 0, len(tlpSettingDefs))
	for _, def := range tlpSettingDefs {
		row := tlpSettingRow{ID: def.ID, Label: def.Label, Group: def.Group,
			Default: def.Default, Value: def.Default, Source: "TLP default", Options: def.Options, Editable: true}
		if configured, ok := values[def.ID]; ok {
			row.Value, row.Source = configured.Value, configured.Source
			if configured.Source != ryokuTLPConfig {
				row.Editable = false
				row.Reason = "external-config"
			}
		}
		rows = append(rows, row)
	}
	return rows
}

func printTLPSettings() error {
	if inspectPowerBackend().Active != "tlp" {
		return fmt.Errorf("TLP and tlp-pd must be active")
	}
	values, err := tlpConfigSources()
	if err != nil {
		return err
	}
	rows := tlpSettingRows(values)
	if _, err := os.Stat(tlpASPMPolicyPath); err != nil {
		for i := range rows {
			if strings.HasPrefix(rows[i].ID, "PCIE_ASPM_") {
				rows[i].Editable = false
				rows[i].Reason = "unsupported-device"
			}
		}
	}
	return printJSON(rows)
}

func validTLPSetting(id, value string) bool {
	for _, def := range tlpSettingDefs {
		if id == def.ID {
			for _, option := range def.Options {
				if value == option {
					return true
				}
			}
		}
	}
	return false
}

func setTLPSetting(id, value string) error {
	if !validTLPSetting(id, value) {
		return fmt.Errorf("unsupported TLP setting or value")
	}
	if os.Geteuid() != 0 {
		return escalateSelf("power", "set", id, value)
	}
	lock, err := os.OpenFile("/run/ryoku-power-backend.lock", os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}
	defer lock.Close()
	if err := unix.Flock(int(lock.Fd()), unix.LOCK_EX); err != nil {
		return err
	}
	defer unix.Flock(int(lock.Fd()), unix.LOCK_UN)
	if inspectPowerBackend().Active != "tlp" {
		return fmt.Errorf("TLP and tlp-pd must be active")
	}
	if strings.HasPrefix(id, "PCIE_ASPM_") {
		if _, err := os.Stat(tlpASPMPolicyPath); err != nil {
			return fmt.Errorf("PCIe ASPM policy is unavailable on this device")
		}
	}
	values, err := tlpConfigSources()
	if err != nil {
		return err
	}
	if existing, ok := values[id]; ok && existing.Source != ryokuTLPConfig {
		return fmt.Errorf("%s is managed in %s", id, existing.Source)
	}
	owned, err := readTLPAssignments(ryokuTLPConfig)
	if err != nil {
		return err
	}
	old, readErr := os.ReadFile(ryokuTLPConfig)
	if readErr != nil && !os.IsNotExist(readErr) {
		return readErr
	}
	owned[id] = value
	if err := writeRyokuTLPConfig(owned); err != nil {
		return err
	}
	if err := exec.Command("tlp", "start").Run(); err != nil {
		if readErr == nil {
			if restoreErr := writeRawRyokuTLPConfig(old); restoreErr != nil {
				return fmt.Errorf("TLP apply failed: %w; config rollback failed: %v", err, restoreErr)
			}
		} else {
			if restoreErr := os.Remove(ryokuTLPConfig); restoreErr != nil {
				return fmt.Errorf("TLP apply failed: %w; config rollback failed: %v", err, restoreErr)
			}
		}
		if restoreErr := exec.Command("tlp", "start").Run(); restoreErr != nil {
			return fmt.Errorf("TLP apply failed: %w; config restored, but reapply failed: %v", err, restoreErr)
		}
		return fmt.Errorf("TLP apply failed: %w; Ryoku config restored", err)
	}
	return nil
}

func writeRyokuTLPConfig(values map[string]string) error {
	if err := os.MkdirAll(filepath.Dir(ryokuTLPConfig), 0o755); err != nil {
		return err
	}
	var b strings.Builder
	b.WriteString("# Managed by Ryoku Hub. Edit through Graphics & Power.\n")
	for _, def := range tlpSettingDefs {
		if value, ok := values[def.ID]; ok {
			if !validTLPSetting(def.ID, value) {
				return fmt.Errorf("invalid stored value for %s", def.ID)
			}
			fmt.Fprintf(&b, "%s=%s\n", def.ID, value)
		}
	}
	return writeRawRyokuTLPConfig([]byte(b.String()))
}

func writeRawRyokuTLPConfig(content []byte) error {
	f, err := os.CreateTemp(filepath.Dir(ryokuTLPConfig), ".ryoku-tlp-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if err := f.Chmod(0o600); err != nil {
		f.Close()
		return err
	}
	if _, err := f.Write(content); err != nil {
		f.Close()
		return err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return os.Rename(f.Name(), ryokuTLPConfig)
}
