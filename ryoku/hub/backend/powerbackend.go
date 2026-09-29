package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/godbus/dbus/v5"
	"golang.org/x/sys/unix"
)

type powerBackendReport struct {
	Active         string              `json:"active"`
	PPDInstalled   bool                `json:"ppdInstalled"`
	TLPInstalled   bool                `json:"tlpInstalled"`
	TLPPDInstalled bool                `json:"tlpPdInstalled"`
	TLPVersion     string              `json:"tlpVersion,omitempty"`
	Conflicts      []string            `json:"conflicts"`
	Healthy        bool                `json:"healthy"`
	Pending        *powerSwitchJournal `json:"pending,omitempty"`
	JournalError   string              `json:"journalError,omitempty"`
}

var powerUnits = map[string][]string{
	"ppd": {"power-profiles-daemon.service"},
	"tlp": {"tlp.service", "tlp-pd.service"},
}

func powerUnitActive(name string) bool {
	return exec.Command("systemctl", "is-active", "--quiet", name).Run() == nil
}

func powerUnitEnabled(name string) bool {
	return exec.Command("systemctl", "is-enabled", "--quiet", name).Run() == nil
}

func powerPackageInstalled(name string) bool {
	return exec.Command("pacman", "-Qq", name).Run() == nil
}

func inspectPowerBackend() powerBackendReport {
	ppd := powerUnitActive(powerUnits["ppd"][0])
	tlp := powerUnitActive(powerUnits["tlp"][0])
	tlppd := powerUnitActive(powerUnits["tlp"][1])
	r := powerBackendReport{
		PPDInstalled:   powerPackageInstalled("power-profiles-daemon"),
		TLPInstalled:   powerPackageInstalled("tlp"),
		TLPPDInstalled: powerPackageInstalled("tlp-pd"),
	}
	if r.TLPInstalled {
		if out, err := exec.Command("pacman", "-Q", "--qf", "%v", "tlp").Output(); err == nil {
			r.TLPVersion = strings.TrimSpace(string(out))
		}
	}
	pending, err := readPowerSwitchJournal()
	r.Pending = pending
	if err != nil {
		r.JournalError = err.Error()
	}
	switch {
	case ppd && (tlp || tlppd):
		r.Active = "conflict"
	case ppd:
		r.Active, r.Healthy = "ppd", r.PPDInstalled
	case tlp && tlppd:
		r.Active, r.Healthy = "tlp", r.TLPInstalled && r.TLPPDInstalled
	case tlp || tlppd:
		r.Active = "incomplete"
	default:
		r.Active = "none"
	}
	for _, manager := range []string{"auto-cpufreq.service", "tuned.service"} {
		if powerUnitActive(manager) {
			r.Conflicts = append(r.Conflicts, manager)
			r.Active, r.Healthy = "conflict", false
		}
	}
	if powerPackageInstalled("asusctl") {
		r.Conflicts = append(r.Conflicts, "asusctl (package conflict with TLP)")
	}
	if r.Healthy && verifyPowerProfileBus() != nil {
		r.Healthy = false
	}
	return r
}

func verifyPowerProfileBus() error {
	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return err
	}
	defer conn.Close()
	const iface = "org.freedesktop.UPower.PowerProfiles"
	obj := conn.Object(iface, dbus.ObjectPath("/org/freedesktop/UPower/PowerProfiles"))
	active, err := obj.GetProperty(iface + ".ActiveProfile")
	if err != nil {
		return err
	}
	if name, ok := active.Value().(string); !ok || name == "" {
		return fmt.Errorf("power profile bus has no active profile")
	}
	return nil
}

func runPowerBackend(args []string) error {
	if len(args) == 1 && args[0] == "recover" {
		if os.Geteuid() != 0 {
			return escalateSelf("power", "recover")
		}
		return recoverPowerBackend()
	}
	if len(args) == 1 && args[0] == "state" {
		frame, err := shellPowerRequest("subscribe powerprofiles")
		if err != nil {
			return err
		}
		fmt.Println(frame)
		return nil
	}
	if len(args) == 2 && args[0] == "preset" {
		payload, _ := json.Marshal(map[string]string{"id": args[1]})
		frame, err := shellPowerRequest("call powerprofiles.setPreset " + string(payload))
		if err != nil {
			return err
		}
		var result struct {
			OK    bool   `json:"ok"`
			Error string `json:"error"`
		}
		if err := json.Unmarshal([]byte(frame), &result); err != nil {
			return err
		}
		if !result.OK {
			return fmt.Errorf("%s", result.Error)
		}
		return nil
	}
	if len(args) == 1 && args[0] == "status" {
		return printJSON(inspectPowerBackend())
	}
	if len(args) == 1 && args[0] == "settings" {
		return printTLPSettings()
	}
	if len(args) == 3 && args[0] == "set" {
		return setTLPSetting(args[1], args[2])
	}
	if len(args) != 2 || args[0] != "switch" || (args[1] != "ppd" && args[1] != "tlp") {
		return fmt.Errorf("power needs status|state|preset <id>|settings|set <id> <value>|switch ppd|tlp|recover")
	}
	if os.Geteuid() != 0 {
		return escalateSelf("power", "switch", args[1])
	}
	return switchPowerBackend(args[1])
}

func shellPowerRequest(request string) (string, error) {
	dir := os.Getenv("XDG_RUNTIME_DIR")
	if dir == "" {
		dir = "/tmp"
	}
	conn, err := net.DialTimeout("unix", filepath.Join(dir, "ryoku-shell.sock"), 2*time.Second)
	if err != nil {
		return "", err
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(5 * time.Second))
	if _, err := fmt.Fprintln(conn, request); err != nil {
		return "", err
	}
	frame, err := bufio.NewReader(conn).ReadString('\n')
	return strings.TrimSpace(frame), err
}

func switchPowerBackend(target string) error {
	lock, err := os.OpenFile("/run/ryoku-power-backend.lock", os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}
	defer lock.Close()
	if err := unix.Flock(int(lock.Fd()), unix.LOCK_EX); err != nil {
		return err
	}
	defer unix.Flock(int(lock.Fd()), unix.LOCK_UN)
	if existing, err := readPowerSwitchJournal(); err != nil {
		return err
	} else if existing != nil {
		return fmt.Errorf("unfinished power switch (%s); run ryoku-hub power recover", existing.Stage)
	}

	before := inspectPowerBackend()
	if before.Active == target && before.Healthy {
		return nil
	}
	if before.Active != "ppd" && before.Active != "tlp" {
		return fmt.Errorf("cannot switch from %s: restore a single healthy backend first", before.Active)
	}
	if len(before.Conflicts) > 0 && target == "tlp" {
		return fmt.Errorf("TLP setup blocked: %s", strings.Join(before.Conflicts, ", "))
	}
	wasEnabled := make(map[string]bool)
	for _, unit := range powerUnits[before.Active] {
		wasEnabled[unit] = powerUnitEnabled(unit)
	}
	// Download both directions while the working backend is still running. A
	// failed mirror or offline machine then leaves the service untouched.
	if err := powerPacman("-Sw", "--noconfirm", "power-profiles-daemon"); err != nil {
		return fmt.Errorf("cache PPD for recovery: %w", err)
	}
	if target == "tlp" {
		if err := powerPacman("-Sw", "--noconfirm", "tlp", "tlp-pd"); err != nil {
			return fmt.Errorf("download TLP packages: %w", err)
		}
	} else if err := powerPacman("-Sw", "--noconfirm", "tlp", "tlp-pd"); err != nil {
		return fmt.Errorf("cache TLP for recovery: %w", err)
	}
	j := newPowerSwitchJournal(before.Active, target, wasEnabled)
	if err := writePowerSwitchJournal(j); err != nil {
		return err
	}
	if err := advancePowerSwitchJournal(j, "stopping-old"); err != nil {
		return err
	}
	for _, unit := range powerUnits[before.Active] {
		if err := powerSystemctl("disable", "--now", unit); err != nil {
			return rollbackPowerBackend(j, fmt.Errorf("stop %s: %w", unit, err))
		}
	}
	if err := advancePowerSwitchJournal(j, "packages"); err != nil {
		return rollbackPowerBackend(j, err)
	}
	if err := replacePowerPackages(before.Active); err != nil {
		return rollbackPowerBackend(j, err)
	}
	if err := advancePowerSwitchJournal(j, "starting-new"); err != nil {
		return rollbackPowerBackend(j, err)
	}
	for _, unit := range powerUnits[target] {
		if err := powerSystemctl("enable", "--now", unit); err != nil {
			return rollbackPowerBackend(j, fmt.Errorf("start %s: %w", unit, err))
		}
	}
	if err := advancePowerSwitchJournal(j, "verifying"); err != nil {
		return rollbackPowerBackend(j, err)
	}
	var after powerBackendReport
	for attempt := 0; attempt < 10; attempt++ {
		after = inspectPowerBackend()
		if after.Active == target && after.Healthy {
			break
		}
		time.Sleep(300 * time.Millisecond)
	}
	if after.Active != target || !after.Healthy || (target == "tlp" && len(after.Conflicts) > 0) {
		return rollbackPowerBackend(j, fmt.Errorf("verification found %s", after.Active))
	}
	if err := advancePowerSwitchJournal(j, "committed"); err != nil {
		return err
	}
	return os.Remove(powerJournalPath)
}

func replacePowerPackages(from string) error {
	var remove, install []string
	if from == "ppd" {
		remove, install = []string{"power-profiles-daemon"}, []string{"tlp", "tlp-pd"}
	} else {
		remove, install = []string{"tlp-pd", "tlp"}, []string{"power-profiles-daemon"}
	}
	if err := powerPacman(append([]string{"-R", "--noconfirm"}, remove...)...); err != nil {
		return fmt.Errorf("remove %s: %w", strings.Join(remove, ", "), err)
	}
	if err := powerPacman(append([]string{"-S", "--noconfirm", "--needed"}, install...)...); err != nil {
		return fmt.Errorf("install %s: %w", strings.Join(install, ", "), err)
	}
	return nil
}

func rollbackPowerBackend(j *powerSwitchJournal, cause error) error {
	previous, wasEnabled := j.From, j.WasEnabled
	j.LastError = cause.Error()
	if err := advancePowerSwitchJournal(j, "rolling-back"); err != nil {
		return fmt.Errorf("%w; cannot record recovery: %v", cause, err)
	}
	var packages []string
	if previous == "ppd" {
		packages = []string{"power-profiles-daemon"}
	} else {
		packages = []string{"tlp", "tlp-pd"}
	}
	var recovery []error
	for _, unit := range powerUnits[oppositePowerBackend(previous)] {
		packageName := strings.TrimSuffix(unit, ".service")
		if !powerPackageInstalled(packageName) {
			continue
		}
		if err := powerSystemctl("disable", "--now", unit); err != nil {
			recovery = append(recovery, err)
		}
	}
	var remove []string
	if previous == "ppd" {
		remove = []string{"tlp-pd", "tlp"}
	} else {
		remove = []string{"power-profiles-daemon"}
	}
	var installed []string
	for _, name := range remove {
		if powerPackageInstalled(name) {
			installed = append(installed, name)
		}
	}
	if len(installed) > 0 {
		if err := powerPacman(append([]string{"-R", "--noconfirm"}, installed...)...); err != nil {
			recovery = append(recovery, err)
		}
	}
	if err := powerPacman(append([]string{"-S", "--noconfirm", "--needed"}, packages...)...); err != nil {
		recovery = append(recovery, err)
	}
	for _, unit := range powerUnits[previous] {
		action := "start"
		if wasEnabled[unit] {
			action = "enable"
		}
		args := []string{action}
		if action == "enable" {
			args = append(args, "--now")
		}
		args = append(args, unit)
		if err := powerSystemctl(args...); err != nil {
			recovery = append(recovery, err)
		}
	}
	if len(recovery) > 0 {
		j.LastError = errors.Join(recovery...).Error()
		_ = advancePowerSwitchJournal(j, "rollback-incomplete")
		return fmt.Errorf("switch failed: %w; recovery incomplete: %v", cause, errors.Join(recovery...))
	}
	if err := os.Remove(powerJournalPath); err != nil {
		return fmt.Errorf("%w; %s restored, but journal cleanup failed: %v", cause, previous, err)
	}
	return fmt.Errorf("switch failed: %w; %s restored", cause, previous)
}

func recoverPowerBackend() error {
	lock, err := os.OpenFile("/run/ryoku-power-backend.lock", os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return err
	}
	defer lock.Close()
	if err := unix.Flock(int(lock.Fd()), unix.LOCK_EX); err != nil {
		return err
	}
	defer unix.Flock(int(lock.Fd()), unix.LOCK_UN)
	j, err := readPowerSwitchJournal()
	if err != nil {
		return err
	}
	if j == nil {
		return nil
	}
	if j.Stage == "committed" {
		state := inspectPowerBackend()
		if state.Active == j.Target && state.Healthy {
			return os.Remove(powerJournalPath)
		}
	}
	return rollbackPowerBackend(j, fmt.Errorf("recovering interrupted switch from %s to %s", j.From, j.Target))
}

func oppositePowerBackend(backend string) string {
	if backend == "ppd" {
		return "tlp"
	}
	return "ppd"
}

func powerSystemctl(args ...string) error {
	out, err := exec.Command("systemctl", args...).CombinedOutput()
	if err != nil {
		return fmt.Errorf("systemctl %s: %w: %s", strings.Join(args, " "), err, strings.TrimSpace(string(out)))
	}
	return nil
}

func powerPacman(args ...string) error {
	cmd := exec.Command("pacman", args...)
	cmd.Stdout, cmd.Stderr = os.Stdout, os.Stderr
	return cmd.Run()
}
