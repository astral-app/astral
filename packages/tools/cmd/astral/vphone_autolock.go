package main

import (
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"time"

	"howett.net/plist"
)

const (
	userConfigurationProfiles = "/var/mobile/Library/UserConfigurationProfiles"
	neverLock                 = 2147483647
)

func withNeverLock(settings map[string]any) map[string]any {
	if settings == nil {
		settings = map[string]any{}
	}
	restricted, _ := settings["restrictedValue"].(map[string]any)
	if restricted == nil {
		restricted = map[string]any{}
	}
	inactivity, _ := restricted["maxInactivity"].(map[string]any)
	if inactivity == nil {
		inactivity = map[string]any{}
	}
	inactivity["value"] = neverLock
	restricted["maxInactivity"] = inactivity
	settings["restrictedValue"] = restricted
	return settings
}

func disableAutoLock(client vphoneClient, name string) error {
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	for _, file := range []string{"EffectiveUserSettings.plist", "UserSettings.plist"} {
		remote := path.Join(userConfigurationProfiles, file)
		local := filepath.Join(work, file)
		settings := map[string]any{}
		if err := client.pull(remote, local); err == nil {
			if data, err := os.ReadFile(local); err == nil {
				plist.Unmarshal(data, &settings)
			}
		}
		encoded, err := plist.Marshal(withNeverLock(settings), plist.BinaryFormat)
		if err != nil {
			return err
		}
		if err := os.WriteFile(local, encoded, 0o644); err != nil {
			return err
		}
		if err := client.push(local, remote); err != nil {
			return fmt.Errorf("write %s: %w", remote, err)
		}
	}
	output, err := vphoneRun(name, "killall -9 profiled SpringBoard", true)
	if err != nil {
		return fmt.Errorf("restart profiled and SpringBoard: %w\n%s", err, output)
	}
	time.Sleep(3 * time.Second)
	client.call("screen.unlock", map[string]any{"timeout_ms": 15000}, nil)
	var state struct {
		Never bool `json:"never"`
	}
	if err := client.call("display.auto_lock", nil, &state); err != nil {
		return err
	}
	if !state.Never {
		return errors.New("auto-lock is still on after writing the user settings")
	}
	return nil
}
