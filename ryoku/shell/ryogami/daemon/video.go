package main

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"sync"
	"syscall"
	"time"
)

// videoPlayer drives video wallpapers through mpvpaper. Each target output gets
// its own mpvpaper process (put in its own process group so the whole tree dies
// on Stop); the all-outputs case is a single process using mpvpaper's "*"
// selector. This replaces the retired skwd paper renderer for the video path.
type videoPlayer struct {
	mu      sync.Mutex
	procs   []*exec.Cmd
	log     *os.File
	path    string
	outputs []string
}

func newVideoPlayer() *videoPlayer { return &videoPlayer{} }

// mpvpaperArgs builds the argv (after the binary name) for one mpvpaper process:
// -o forwards a space-separated mpv option string, then the output selector and
// the video path. Muted playback drops audio entirely; otherwise the volume is
// pinned. hwdec=auto offloads decoding and panscan=1.0 fills the screen (cover).
func mpvpaperArgs(output, path string, muted bool, volume int) []string {
	opts := "loop hwdec=auto panscan=1.0"
	if muted {
		opts += " no-audio"
	} else {
		opts += fmt.Sprintf(" volume=%d", volume)
	}
	return []string{"-o", opts, output, path}
}

// muteVol resolves the per-output mute and volume, defaulting to unmuted at full
// volume when the maps carry no entry for the key.
func muteVol(key string, mute map[string]bool, volume map[string]int) (bool, int) {
	vol := 100
	if v, ok := volume[key]; ok {
		vol = v
	}
	return mute[key], vol
}

// Play stops any current playback then spawns fresh mpvpaper processes. An empty
// outputs slice or one containing "*" means every output: a single process with
// the "*" selector, keyed by the "*" mute/volume entry.
func (p *videoPlayer) Play(outputs []string, path string, mute map[string]bool, volume map[string]int) {
	p.Stop()

	all := len(outputs) == 0
	for _, o := range outputs {
		if o == "*" {
			all = true
			break
		}
	}

	p.mu.Lock()
	defer p.mu.Unlock()

	if f, err := os.OpenFile(managedLogPath("video"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644); err == nil {
		p.log = f
	}

	var targets []string
	if all {
		targets = []string{"*"}
	} else {
		targets = append(targets, outputs...)
	}

	for _, out := range targets {
		muted, vol := muteVol(out, mute, volume)
		p.spawnLocked(out, path, muted, vol)
	}

	p.path = path
	p.outputs = targets
}

// spawnLocked launches one mpvpaper process; caller holds the lock. A crash while
// the player still believes it should be running is logged to stderr, but there
// is deliberately no auto-restart loop.
func (p *videoPlayer) spawnLocked(output, path string, muted bool, volume int) {
	cmd := exec.Command("mpvpaper", mpvpaperArgs(output, path, muted, volume)...)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	cmd.Stdin = nil
	if p.log != nil {
		cmd.Stdout = p.log
		cmd.Stderr = p.log
	}
	if err := cmd.Start(); err != nil {
		fmt.Fprintf(os.Stderr, "ryogami: launch mpvpaper: %v\n", err)
		return
	}
	p.procs = append(p.procs, cmd)
	go func() {
		err := cmd.Wait()
		p.mu.Lock()
		defer p.mu.Unlock()
		// Still tracked means Stop did not remove it: an unexpected exit.
		if p.removeLocked(cmd) {
			fmt.Fprintf(os.Stderr, "ryogami: mpvpaper on %q exited unexpectedly: %v\n", output, err)
		}
	}()
}

// removeLocked drops cmd from the tracked set, returning whether it was present.
func (p *videoPlayer) removeLocked(cmd *exec.Cmd) bool {
	for i, c := range p.procs {
		if c == cmd {
			p.procs = append(p.procs[:i], p.procs[i+1:]...)
			return true
		}
	}
	return false
}

// Stop kills every spawned mpvpaper (whole process group) and, mirroring how
// process.go clears stale wall-ui, pkills orphans left by a previous daemon.
func (p *videoPlayer) Stop() {
	p.mu.Lock()
	procs := p.procs
	p.procs = nil
	log := p.log
	p.log = nil
	p.path = ""
	p.outputs = nil
	p.mu.Unlock()

	for _, cmd := range procs {
		if cmd.Process != nil {
			_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
		}
	}
	if log != nil {
		_ = log.Close()
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = exec.CommandContext(ctx, "pkill", "-f", "mpvpaper").Run()
}

func (p *videoPlayer) Playing() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return len(p.procs) > 0
}

func (p *videoPlayer) Current() (path string, outputs []string) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.path, append([]string(nil), p.outputs...)
}
