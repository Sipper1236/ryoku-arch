package main

import "os/exec"

type powerPresetChoice struct {
	ID        string `json:"id"`
	Available bool   `json:"available"`
	Reason    string `json:"reason,omitempty"`
}

// tlp-pd owns the same D-Bus name as PPD. Service ownership decides whether
// Ryoku's PPD-specific sysfs hook may run after a profile change.
func activePowerBackend() string {
	if exec.Command("systemctl", "is-active", "--quiet", "tlp-pd.service").Run() == nil {
		return "tlp"
	}
	if exec.Command("systemctl", "is-active", "--quiet", "tlp.service").Run() == nil ||
		exec.Command("systemctl", "is-enabled", "--quiet", "tlp.service").Run() == nil {
		return "tlp-no-profile-daemon"
	}
	return "ppd"
}

func powerPresetChoices(profiles []string) []powerPresetChoice {
	choices := make([]powerPresetChoice, 0, 4)
	for _, id := range []string{"power-saver", "balanced", "balance-performance", "performance"} {
		available := false
		for _, profile := range profiles {
			if profile == id {
				available = true
				break
			}
		}
		choice := powerPresetChoice{ID: id, Available: available}
		if !available {
			choice.Reason = "profile-unavailable"
		}
		choices = append(choices, choice)
	}
	return choices
}
