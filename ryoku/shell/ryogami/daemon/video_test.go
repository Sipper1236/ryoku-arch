package main

import (
	"bufio"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestVideoMpvpaperArgs(t *testing.T) {
	cases := []struct {
		name   string
		output string
		path   string
		muted  bool
		volume int
		want   []string
	}{
		{
			name:   "muted per-output",
			output: "DP-1",
			path:   "/w/clip.mp4",
			muted:  true,
			volume: 100,
			want:   []string{"-o", "loop hwdec=auto panscan=1.0 no-audio", "DP-1", "/w/clip.mp4"},
		},
		{
			name:   "volume per-output",
			output: "HDMI-A-1",
			path:   "/w/clip.mp4",
			muted:  false,
			volume: 45,
			want:   []string{"-o", "loop hwdec=auto panscan=1.0 volume=45", "HDMI-A-1", "/w/clip.mp4"},
		},
		{
			name:   "all outputs volume",
			output: "*",
			path:   "/w/clip.mp4",
			muted:  false,
			volume: 100,
			want:   []string{"-o", "loop hwdec=auto panscan=1.0 volume=100", "*", "/w/clip.mp4"},
		},
		{
			name:   "all outputs muted",
			output: "*",
			path:   "/w/clip.mp4",
			muted:  true,
			volume: 0,
			want:   []string{"-o", "loop hwdec=auto panscan=1.0 no-audio", "*", "/w/clip.mp4"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := mpvpaperArgs(tc.output, tc.path, tc.muted, tc.volume)
			if !reflect.DeepEqual(got, tc.want) {
				t.Fatalf("argv = %q, want %q", got, tc.want)
			}
			t.Logf("mpvpaper argv: mpvpaper %s", strings.Join(got, " "))
		})
	}
}

// fakeMpvpaper installs a mpvpaper shell script on PATH that records its argv to
// a marker file, then either exits immediately (crash) or sleeps (steady play).
func fakeMpvpaper(t *testing.T, marker string, exitFast bool) {
	t.Helper()
	dir := t.TempDir()
	body := "#!/bin/sh\nprintf '%s\\n' \"$*\" >> " + marker + "\n"
	if exitFast {
		body += "exit 0\n"
	} else {
		body += "sleep 30\n"
	}
	script := filepath.Join(dir, "mpvpaper")
	if err := os.WriteFile(script, []byte(body), 0o755); err != nil {
		t.Fatal(err)
	}
	old := os.Getenv("PATH")
	t.Setenv("PATH", dir+":"+old)
	t.Setenv("XDG_CACHE_HOME", t.TempDir())
}

func readMarker(t *testing.T, marker string) []string {
	t.Helper()
	data, err := os.ReadFile(marker)
	if err != nil {
		return nil
	}
	var lines []string
	sc := bufio.NewScanner(strings.NewReader(string(data)))
	for sc.Scan() {
		if l := strings.TrimSpace(sc.Text()); l != "" {
			lines = append(lines, l)
		}
	}
	return lines
}

func waitFor(t *testing.T, cond func() bool) bool {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		if cond() {
			return true
		}
		time.Sleep(10 * time.Millisecond)
	}
	return cond()
}

func TestVideoPlayStopLifecycle(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawned")
	fakeMpvpaper(t, marker, false)

	p := newVideoPlayer()
	mute := map[string]bool{"DP-1": true}
	volume := map[string]int{"DP-2": 60}
	p.Play([]string{"DP-1", "DP-2"}, "/w/clip.mp4", mute, volume)

	if !p.Playing() {
		t.Fatal("Playing() = false after Play")
	}
	path, outs := p.Current()
	if path != "/w/clip.mp4" || !reflect.DeepEqual(outs, []string{"DP-1", "DP-2"}) {
		t.Fatalf("Current() = %q %q", path, outs)
	}

	if !waitFor(t, func() bool { return len(readMarker(t, marker)) == 2 }) {
		t.Fatalf("expected 2 spawns, got %q", readMarker(t, marker))
	}
	// The two processes run concurrently and append in a racy order, so compare
	// as an unordered set; spawn order itself is fixed by the loop in Play.
	lines := readMarker(t, marker)
	want := []string{
		"-o loop hwdec=auto panscan=1.0 no-audio DP-1 /w/clip.mp4",
		"-o loop hwdec=auto panscan=1.0 volume=60 DP-2 /w/clip.mp4",
	}
	sort.Strings(lines)
	sort.Strings(want)
	if !reflect.DeepEqual(lines, want) {
		t.Fatalf("spawn argv = %q, want %q", lines, want)
	}
	for _, l := range lines {
		t.Logf("spawned: mpvpaper %s", l)
	}

	// Snapshot the process-group leader pids so we can prove Stop reaped them.
	p.mu.Lock()
	var pids []int
	for _, c := range p.procs {
		pids = append(pids, c.Process.Pid)
	}
	p.mu.Unlock()

	p.Stop()
	if p.Playing() {
		t.Fatal("Playing() = true after Stop")
	}
	if path, outs := p.Current(); path != "" || outs != nil {
		t.Fatalf("Current() after Stop = %q %q", path, outs)
	}
	for _, pid := range pids {
		gone := waitFor(t, func() bool { return syscall.Kill(pid, 0) == syscall.ESRCH })
		if !gone {
			t.Fatalf("pid %d still alive after Stop", pid)
		}
	}
}

func TestVideoPlayAllOutputs(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawned")
	fakeMpvpaper(t, marker, false)

	p := newVideoPlayer()
	defer p.Stop()
	p.Play(nil, "/w/clip.mp4", nil, nil)

	if !waitFor(t, func() bool { return len(readMarker(t, marker)) == 1 }) {
		t.Fatalf("expected 1 spawn for all-outputs, got %q", readMarker(t, marker))
	}
	lines := readMarker(t, marker)
	want := "-o loop hwdec=auto panscan=1.0 volume=100 * /w/clip.mp4"
	if lines[0] != want {
		t.Fatalf("spawn argv = %q, want %q", lines[0], want)
	}
	t.Logf("spawned: mpvpaper %s", lines[0])

	if _, outs := p.Current(); !reflect.DeepEqual(outs, []string{"*"}) {
		t.Fatalf("Current outputs = %q, want [*]", outs)
	}
}

func TestVideoCrashResilience(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "spawned")
	fakeMpvpaper(t, marker, true) // exits immediately

	// Capture stderr to confirm the unexpected-exit log fires without a restart.
	oldErr := os.Stderr
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	os.Stderr = w
	defer func() { os.Stderr = oldErr }()

	p := newVideoPlayer()
	defer p.Stop()
	p.Play([]string{"DP-1"}, "/w/clip.mp4", nil, nil)

	// The fast-exiting fake makes the player fall idle with no auto-restart.
	if !waitFor(t, func() bool { return !p.Playing() }) {
		t.Fatal("Playing() stayed true after crash")
	}

	_ = w.Close()
	os.Stderr = oldErr
	out, _ := io.ReadAll(r)
	if !strings.Contains(string(out), "exited unexpectedly") {
		t.Fatalf("stderr missing crash log, got %q", string(out))
	}
}
