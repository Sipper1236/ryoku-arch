package main

import "testing"

func TestPowerPresetChoices(t *testing.T) {
	choices := powerPresetChoices([]string{"power-saver", "balanced", "performance"})
	if len(choices) != 4 {
		t.Fatalf("got %d choices", len(choices))
	}
	for i, id := range []string{"power-saver", "balanced", "balance-performance", "performance"} {
		if choices[i].ID != id {
			t.Fatalf("choice %d = %q, want %q", i, choices[i].ID, id)
		}
		if choices[i].Available == (id == "balance-performance") {
			t.Errorf("availability for %q is wrong", id)
		}
	}
}
