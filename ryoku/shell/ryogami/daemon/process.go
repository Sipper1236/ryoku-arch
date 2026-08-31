package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
)

// managedProcess supervises the wall-ui picker: one quickshell instance
// launched with the install dir handed through RYOGAMI_WALL_INSTALL, output
// captured to the cache log. Toggle kills or launches, mirroring the Rust
// daemon; headless mode (tests) spawns nothing.
type managedProcess struct {
	mu   sync.Mutex
	cmd  *exec.Cmd
	qml  string
	envK string
}

func headless() bool { return os.Getenv("RYOGAMI_HEADLESS") != "" }

func newWallUIProcess() *managedProcess {
	return &managedProcess{qml: resolveShellQML(), envK: "RYOGAMI_WALL_INSTALL"}
}

// resolveShellQML finds the wall-ui entry point: env override first (the dev
// deploy sets it on the unit), then the packaged default.
func resolveShellQML() string {
	if p := os.Getenv("RYOGAMI_SHELL_QML"); p != "" {
		return p
	}
	if p := "shell.qml"; fileExists(p) {
		abs, err := filepath.Abs(p)
		if err == nil {
			return abs
		}
	}
	return "/usr/share/ryogami/shell.qml"
}

func (m *managedProcess) running() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.runningLocked()
}

func (m *managedProcess) runningLocked() bool {
	if m.cmd == nil || m.cmd.Process == nil {
		return false
	}
	if m.cmd.ProcessState != nil {
		m.cmd = nil
		return false
	}
	return true
}

func (m *managedProcess) launch() {
	if headless() {
		return
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.runningLocked() {
		return
	}
	// A leftover instance from a previous daemon holds the surface; clear it.
	_ = exec.Command("pkill", "-f", "quickshell .*"+m.qml).Run()
	cmd := exec.Command("quickshell", "-p", m.qml)
	cmd.Env = append(os.Environ(), m.envK+"="+filepath.Dir(m.qml))
	cmd.Stdin = nil
	if log := managedLogPath("wall-ui"); log != "" {
		if f, err := os.OpenFile(log, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644); err == nil {
			cmd.Stdout = f
			cmd.Stderr = f
		}
	}
	if err := cmd.Start(); err != nil {
		fmt.Fprintf(os.Stderr, "ryogami: launch wall-ui: %v\n", err)
		return
	}
	m.cmd = cmd
	go func() { _ = cmd.Wait() }()
}

func (m *managedProcess) kill() {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.cmd != nil && m.cmd.Process != nil {
		_ = m.cmd.Process.Kill()
		m.cmd = nil
	}
}

func (m *managedProcess) toggle() bool {
	if m.running() {
		m.kill()
		return false
	}
	m.launch()
	return m.running()
}

func managedLogPath(label string) string {
	base := os.Getenv("XDG_CACHE_HOME")
	if base == "" {
		base = filepath.Join(home(), ".cache")
	}
	dir := filepath.Join(base, "ryogami")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return ""
	}
	return filepath.Join(dir, label+".log")
}
