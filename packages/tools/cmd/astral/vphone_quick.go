package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"net/http"
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
	pushed, removed, binaryChanged, err := syncTweakFiles(client, files, path.Join(root, "usr", "lib", "astral"), tweaks)
	if err != nil {
		return err
	}
	if pushed+removed > 0 && !binaryChanged && discordRunning(client) {
		if reply, err := reloadInPlace(); err == nil {
			fmt.Printf("deployed %d changed resource files and reloaded the theme and plugins in place (%s) in %.1fs\n", pushed+removed, reply, time.Since(started).Seconds())
			return nil
		}
	}
	if pushed+removed == 0 && discordRunning(client) {
		fmt.Printf("nothing changed; Discord keeps running (%.1fs)\n", time.Since(started).Seconds())
		return nil
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

func listRemote(client vphoneClient, directory string, depth int, present map[string]bool) {
	if depth > 6 {
		return
	}
	var listing struct {
		Entries []struct {
			Name string `json:"name"`
			Type string `json:"type"`
		} `json:"entries"`
	}
	if client.call("files.list", map[string]any{"path": directory, "limit": 1000}, &listing) != nil {
		return
	}
	for _, entry := range listing.Entries {
		full := path.Join(directory, entry.Name)
		switch entry.Type {
		case "directory", "dir":
			listRemote(client, full, depth+1, present)
		case "link":
		default:
			present[strings.TrimPrefix(full, "/private")] = true
		}
	}
}

func remoteFiles(client vphoneClient, roots ...string) map[string]bool {
	present := map[string]bool{}
	for _, root := range roots {
		listRemote(client, root, 0, present)
	}
	return present
}

func reloadInPlace() (string, error) {
	d := newDevClient()
	if err := d.forward(); err != nil {
		return "", err
	}
	reply, err := d.request(http.MethodPost, "/reload", map[string]any{})
	if err != nil {
		return "", err
	}
	if !strings.Contains(string(reply), "\"theme\"") {
		return "", errors.New(strings.TrimSpace(string(reply)))
	}
	return strings.TrimSpace(string(reply)), nil
}

func syncTweakFiles(client vphoneClient, files []deployFile, resources string, tweaks string) (int, int, bool, error) {
	cache := map[string]string{}
	if data, err := os.ReadFile(deployCachePath()); err == nil {
		json.Unmarshal(data, &cache)
	}
	present := remoteFiles(client, resources)
	if entries, err := client.list(tweaks); err == nil {
		for _, entry := range entries {
			present[strings.TrimPrefix(path.Join(tweaks, entry.Name), "/private")] = true
		}
	}
	wanted := map[string]bool{}
	pushed := 0
	binaryChanged := false
	for _, file := range files {
		wanted[file.remote] = true
		hash, err := fileHash(file.local)
		if err != nil {
			return pushed, 0, binaryChanged, err
		}
		if cache[file.remote] == hash && present[strings.TrimPrefix(file.remote, "/private")] {
			continue
		}
		if strings.HasSuffix(file.remote, ".dylib") {
			client.call("files.remove", map[string]any{"path": file.remote}, nil)
		}
		if err := client.push(file.local, file.remote); err != nil {
			return pushed, 0, binaryChanged, err
		}
		cache[file.remote] = hash
		pushed++
		if strings.HasPrefix(file.remote, tweaks+"/") {
			binaryChanged = true
		}
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
		return pushed, removed, binaryChanged, err
	}
	encoded, _ := json.Marshal(cache)
	return pushed, removed, binaryChanged, os.WriteFile(deployCachePath(), encoded, 0o600)
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
