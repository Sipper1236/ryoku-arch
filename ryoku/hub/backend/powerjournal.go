package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

const powerJournalPath = "/var/lib/ryoku/power-backend-transaction.json"

type powerSwitchJournal struct {
	Schema     int             `json:"schema"`
	From       string          `json:"from"`
	Target     string          `json:"target"`
	Stage      string          `json:"stage"`
	StartedAt  string          `json:"startedAt"`
	WasEnabled map[string]bool `json:"wasEnabled"`
	LastError  string          `json:"lastError,omitempty"`
}

func newPowerSwitchJournal(from, target string, wasEnabled map[string]bool) *powerSwitchJournal {
	return &powerSwitchJournal{Schema: 1, From: from, Target: target,
		Stage: "prepared", StartedAt: time.Now().UTC().Format(time.RFC3339), WasEnabled: wasEnabled}
}

func readPowerSwitchJournal() (*powerSwitchJournal, error) {
	b, err := os.ReadFile(powerJournalPath)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var j powerSwitchJournal
	if err := json.Unmarshal(b, &j); err != nil {
		return nil, err
	}
	if j.Schema != 1 || (j.From != "ppd" && j.From != "tlp") ||
		(j.Target != "ppd" && j.Target != "tlp") || j.From == j.Target || j.WasEnabled == nil {
		return nil, fmt.Errorf("invalid power switch journal")
	}
	return &j, nil
}

func writePowerSwitchJournal(j *powerSwitchJournal) error {
	dir := filepath.Dir(powerJournalPath)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	b, err := json.MarshalIndent(j, "", "  ")
	if err != nil {
		return err
	}
	f, err := os.CreateTemp(dir, ".power-backend-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if err := f.Chmod(0o644); err != nil {
		f.Close()
		return err
	}
	if _, err := f.Write(append(b, '\n')); err != nil {
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
	return os.Rename(f.Name(), powerJournalPath)
}

func advancePowerSwitchJournal(j *powerSwitchJournal, stage string) error {
	j.Stage = stage
	return writePowerSwitchJournal(j)
}
