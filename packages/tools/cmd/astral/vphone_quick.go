package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
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
	return repoPath("packages", "ios", ".theos", "_", "var", "jb")
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
	files, err := tweakFiles(tweaks, path.Join(root, "usr", "lib", "astral"))
	if err != nil {
		return err
	}
	pushed, removed, err := syncTweakFiles(client, files, path.Join(root, "usr", "lib", "astral"), tweaks)
	if err != nil {
		return err
	}
	if _, err := saveVphoneSession(client); err == nil {
		fmt.Println("refreshed the saved login")
	}
	client.call("apps.terminate", map[string]any{"bundle_id": discordBundleID}, nil)
	if err := launchWhenStopped(client); err != nil {
		return err
	}
	fmt.Printf("deployed astral as an ElleKit tweak (%d of %d files changed, %d removed) and relaunched Discord in %.1fs\n", pushed, len(files), removed, time.Since(started).Seconds())
	return nil
}

type deployFile struct {
	local  string
	remote string
}

func tweakFiles(tweaks string, resources string) ([]deployFile, error) {
	staged := stagedTweak()
	libraries := filepath.Join(staged, "Library", "MobileSubstrate", "DynamicLibraries")
	files := []deployFile{
		{filepath.Join(libraries, "astral.dylib"), path.Join(tweaks, "astral.dylib")},
		{filepath.Join(libraries, "astral.plist"), path.Join(tweaks, "astral.plist")},
	}
	local := filepath.Join(staged, "usr", "lib", "astral")
	err := filepath.WalkDir(local, func(file string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil || entry.IsDir() {
			return walkErr
		}
		relative, _ := filepath.Rel(local, file)
		files = append(files, deployFile{file, path.Join(resources, filepath.ToSlash(relative))})
		return nil
	})
	return files, err
}

func deployCachePath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".cache", "astral", "vphone-deploy.json")
}

func fileHash(file string) (string, error) {
	data, err := os.ReadFile(file)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:]), nil
}

func remoteFiles(client vphoneClient, roots ...string) map[string]bool {
	present := map[string]bool{}
	for _, root := range roots {
		cursor := ""
		for {
			params := map[string]any{"root": root, "pattern": "/", "limit": 1000}
			if cursor != "" {
				params["cursor"] = cursor
			}
			var page struct {
				Matches    []string `json:"matches"`
				NextCursor string   `json:"next_cursor"`
			}
			if err := client.call("files.find", params, &page); err != nil {
				break
			}
			for _, match := range page.Matches {
				present[strings.TrimPrefix(match, "/private")] = true
			}
			if page.NextCursor == "" {
				break
			}
			cursor = page.NextCursor
		}
	}
	return present
}

func syncTweakFiles(client vphoneClient, files []deployFile, resources string, tweaks string) (int, int, error) {
	cache := map[string]string{}
	if data, err := os.ReadFile(deployCachePath()); err == nil {
		json.Unmarshal(data, &cache)
	}
	present := remoteFiles(client, resources, tweaks)
	wanted := map[string]bool{}
	pushed := 0
	for _, file := range files {
		wanted[file.remote] = true
		hash, err := fileHash(file.local)
		if err != nil {
			return pushed, 0, err
		}
		if cache[file.remote] == hash && present[strings.TrimPrefix(file.remote, "/private")] {
			continue
		}
		if strings.HasSuffix(file.remote, ".dylib") {
			client.call("files.remove", map[string]any{"path": file.remote}, nil)
		}
		if err := client.push(file.local, file.remote); err != nil {
			return pushed, 0, err
		}
		cache[file.remote] = hash
		pushed++
	}
	removed := 0
	for remote := range present {
		if strings.HasPrefix(remote, strings.TrimPrefix(resources, "/private")+"/") && !wanted[remote] {
			if client.call("files.remove", map[string]any{"path": remote}, nil) == nil {
				delete(cache, remote)
				removed++
			}
		}
	}
	if err := os.MkdirAll(filepath.Dir(deployCachePath()), 0o700); err != nil {
		return pushed, removed, err
	}
	encoded, _ := json.Marshal(cache)
	return pushed, removed, os.WriteFile(deployCachePath(), encoded, 0o600)
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
