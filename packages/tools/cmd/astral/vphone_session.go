package main

import (
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strings"
)

const discordBundleID = "com.hammerandchisel.discord"

type sessionFile struct {
	local  string
	remote string
	pushTo string
}

var sessionFiles = []sessionFile{
	{"keychain.plist", "Documents/astral/session.plist", "Documents/astral/session-import.plist"},
	{"preferences.plist", "Library/Preferences/com.hammerandchisel.discord.plist", ""},
	{"wasm_storage.plist", "Library/Preferences/wasm_storage.plist", ""},
	{"wasm_device.plist", "Library/Preferences/wasm_device.plist", ""},
}

func vphoneSessionDirectory() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "astral", "vphone-session")
}

func discordDataPath(client vphoneClient) (string, error) {
	var info struct {
		DataPath string `json:"data_path"`
	}
	if err := client.call("apps.info", map[string]any{"bundle_id": discordBundleID}, &info); err != nil {
		return "", err
	}
	if info.DataPath == "" {
		return "", errors.New("Discord has no data container on the vphone")
	}
	return info.DataPath, nil
}

func saveVphoneSession(client vphoneClient) (int, error) {
	data, err := discordDataPath(client)
	if err != nil {
		return 0, err
	}
	directory := vphoneSessionDirectory()
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return 0, err
	}
	saved := 0
	for _, file := range sessionFiles {
		staging := filepath.Join(directory, "."+file.local)
		if err := client.pull(path.Join(data, file.remote), staging); err != nil {
			os.Remove(staging)
			continue
		}
		os.Chmod(staging, 0o600)
		if err := os.Rename(staging, filepath.Join(directory, file.local)); err != nil {
			return saved, err
		}
		saved++
	}
	if !fileExists(filepath.Join(directory, "keychain.plist")) {
		return saved, errors.New("no login saved yet; open Discord on the vphone once with this astral build, then send it to the background")
	}
	return saved, nil
}

func restoreVphoneSession(client vphoneClient, name string) (int, error) {
	directory := vphoneSessionDirectory()
	if !fileExists(filepath.Join(directory, "keychain.plist")) {
		return 0, errors.New("no saved login in " + directory + "; run astral vphone session save first")
	}
	data, err := discordDataPath(client)
	if err != nil {
		return 0, err
	}
	restored := 0
	var written []string
	for _, file := range sessionFiles {
		local := filepath.Join(directory, file.local)
		if !fileExists(local) {
			continue
		}
		target := file.remote
		if file.pushTo != "" {
			target = file.pushTo
		}
		remote := path.Join(data, target)
		if err := client.push(local, remote); err != nil {
			return restored, fmt.Errorf("restore %s: %w", file.local, err)
		}
		written = append(written, shellQuote(remote), shellQuote(path.Dir(remote)))
		restored++
	}
	if output, err := vphoneRun(name, "chown mobile:mobile "+strings.Join(written, " "), true); err != nil {
		return restored, fmt.Errorf("hand the restored files to mobile: %w\n%s", err, output)
	}
	return restored, nil
}

func runVphoneSession(client vphoneClient, args []string) error {
	if len(args) != 1 {
		return errors.New("usage: astral vphone session <save|restore>")
	}
	switch args[0] {
	case "save":
		saved, err := saveVphoneSession(client)
		if err != nil {
			return err
		}
		fmt.Printf("saved %d session files to %s\n", saved, vphoneSessionDirectory())
		return nil
	case "restore":
		restored, err := restoreVphoneSession(client, "vphone")
		if err != nil {
			return err
		}
		fmt.Printf("restored %d session files; astral loads the login on Discord's next launch\n", restored)
		return nil
	}
	return errors.New("usage: astral vphone session <save|restore>")
}
