package main

import (
	"errors"
	"fmt"
	"io/fs"
	"path"
	"path/filepath"
	"strings"
	"time"
)

const containersRoot = "/var/containers/Bundle/Application"

type guestEntry struct {
	Name string `json:"name"`
}

func (v vphoneClient) list(directory string) ([]guestEntry, error) {
	var listing struct {
		Entries []guestEntry `json:"entries"`
	}
	err := v.call("files.list", map[string]any{"path": directory, "limit": 1000}, &listing)
	return listing.Entries, err
}

func (v vphoneClient) exists(directory string, name string) bool {
	entries, err := v.list(directory)
	if err != nil {
		return false
	}
	for _, entry := range entries {
		if entry.Name == name {
			return true
		}
	}
	return false
}

func jailbreakRoot(client vphoneClient) (string, error) {
	entries, err := client.list(containersRoot)
	if err != nil {
		return "", err
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name, ".jbroot-") {
			return path.Join(containersRoot, entry.Name), nil
		}
	}
	return "", errors.New("no roothide jailbreak root on the vphone; is the Irisin bootstrap installed?")
}

func discordBundlePath(client vphoneClient) (string, error) {
	var info struct {
		BundlePath string `json:"bundle_path"`
	}
	if err := client.call("apps.info", map[string]any{"bundle_id": discordBundleID}, &info); err != nil {
		return "", err
	}
	return info.BundlePath, nil
}

func stagedTweak() string {
	return repoPath("packages", "loader-ios", ".theos", "_", "var", "jb")
}

func vphoneTweakDeploy(client vphoneClient) error {
	started := time.Now()
	root, err := jailbreakRoot(client)
	if err != nil {
		return err
	}
	tweaks := path.Join(root, "usr", "lib", "TweakInject")
	if !client.exists(path.Join(root, "usr", "lib"), "TweakLoader.dylib") {
		return errors.New("ElleKit is not installed on the vphone; see the vphone section of the vphone skill")
	}
	if bundle, err := discordBundlePath(client); err != nil {
		return fmt.Errorf("Discord is not installed on the vphone; run astral vphone discord --ipa <decrypted Discord.ipa>: %w", err)
	} else if client.exists(path.Join(bundle, "Frameworks"), "astral.dylib") {
		return errors.New("the installed Discord has astral built in; reinstall it stock with astral vphone discord --ipa <decrypted Discord.ipa>")
	}
	if err := buildPackage(); err != nil {
		return err
	}
	staged := stagedTweak()
	dylib := path.Join(tweaks, "astral.dylib")
	client.call("files.remove", map[string]any{"path": dylib}, nil)
	if err := client.push(filepath.Join(staged, "Library", "MobileSubstrate", "DynamicLibraries", "astral.dylib"), dylib); err != nil {
		return err
	}
	if err := client.push(filepath.Join(staged, "Library", "MobileSubstrate", "DynamicLibraries", "astral.plist"), path.Join(tweaks, "astral.plist")); err != nil {
		return err
	}
	resources := path.Join(root, "usr", "lib", "astral")
	client.call("files.remove", map[string]any{"path": resources, "recursive": true}, nil)
	pushed := 2
	local := filepath.Join(staged, "usr", "lib", "astral")
	err = filepath.WalkDir(local, func(file string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil || entry.IsDir() {
			return walkErr
		}
		relative, _ := filepath.Rel(local, file)
		pushed++
		return client.push(file, path.Join(resources, filepath.ToSlash(relative)))
	})
	if err != nil {
		return err
	}
	client.call("apps.terminate", map[string]any{"bundle_id": discordBundleID}, nil)
	if err := launchWhenStopped(client); err != nil {
		return err
	}
	fmt.Printf("deployed astral as an ElleKit tweak (%d files) and relaunched Discord in %.1fs\n", pushed, time.Since(started).Seconds())
	return nil
}

func discordRunning(client vphoneClient) bool {
	var listing struct {
		Processes []struct {
			BundleID string `json:"bundle_id"`
		} `json:"processes"`
	}
	if err := client.call("processes.list", map[string]any{"filter": "Discord"}, &listing); err != nil {
		return false
	}
	for _, process := range listing.Processes {
		if process.BundleID == discordBundleID {
			return true
		}
	}
	return false
}

func launchWhenStopped(client vphoneClient) error {
	for attempt := 0; attempt < 20 && discordRunning(client); attempt++ {
		time.Sleep(150 * time.Millisecond)
	}
	client.call("screen.unlock", map[string]any{"timeout_ms": 10000}, nil)
	var lastErr error
	for attempt := 0; attempt < 10; attempt++ {
		lastErr = client.call("apps.launch", map[string]any{"bundle_id": discordBundleID}, nil)
		time.Sleep(700 * time.Millisecond)
		if lastErr == nil && discordRunning(client) {
			return nil
		}
	}
	if lastErr != nil {
		return lastErr
	}
	return errors.New("Discord did not stay running; read the newest Discord report in /var/mobile/Library/Logs/CrashReporter")
}
