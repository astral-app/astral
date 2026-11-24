package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

const (
	discordAPKTracker     = "https://tracker.vendetta.rocks/tracker"
	discordAPKCertificate = "3c39d23cf9367849a5c699395647fe0e5bfea5a1f1f40d8c717ddc70f8bfa113"
	maximumDiscordAPKSize = 512 << 20
)

var discordAPKSplits = []string{"base", "config.arm64_v8a"}

type discordAPKIndex struct {
	Latest map[string]int `json:"latest"`
}

var trackerClient = &http.Client{Timeout: 10 * time.Minute}

const trackerAttempts = 4

func trackerGet(url string) (*http.Response, error) {
	var lastErr error
	for attempt := 1; attempt <= trackerAttempts; attempt++ {
		response, err := trackerClient.Get(url)
		if err == nil && response.StatusCode < http.StatusInternalServerError {
			return response, nil
		}
		if err == nil {
			response.Body.Close()
			err = fmt.Errorf("HTTP %d", response.StatusCode)
		}
		lastErr = err
		if attempt < trackerAttempts {
			time.Sleep(time.Duration(attempt*attempt) * 5 * time.Second)
		}
	}
	return nil, lastErr
}

func discordVersionCode(channel string) (int, error) {
	response, err := trackerGet(discordAPKTracker + "/index")
	if err != nil {
		return 0, fmt.Errorf("read the Discord APK index: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return 0, fmt.Errorf("read the Discord APK index: HTTP %d", response.StatusCode)
	}
	var index discordAPKIndex
	if err := json.NewDecoder(io.LimitReader(response.Body, 1<<20)).Decode(&index); err != nil {
		return 0, fmt.Errorf("parse the Discord APK index: %w", err)
	}
	code, found := index.Latest[channel]
	if !found || code <= 0 {
		channels := make([]string, 0, len(index.Latest))
		for name := range index.Latest {
			channels = append(channels, name)
		}
		sort.Strings(channels)
		return 0, fmt.Errorf("no Discord %q channel; the tracker lists %s", channel, strings.Join(channels, ", "))
	}
	return code, nil
}

func downloadDiscordSplit(code int, split string, directory string) (string, error) {
	url := fmt.Sprintf("%s/download/%d/%s", discordAPKTracker, code, split)
	response, err := trackerGet(url)
	if err != nil {
		return "", fmt.Errorf("download Discord %d %s: %w", code, split, err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", fmt.Errorf("download Discord %d %s: HTTP %d", code, split, response.StatusCode)
	}
	destination := filepath.Join(directory, split+".apk")
	file, err := os.Create(destination)
	if err != nil {
		return "", err
	}
	written, copyErr := io.Copy(file, io.LimitReader(response.Body, maximumDiscordAPKSize+1))
	closeErr := file.Close()
	if copyErr != nil {
		return "", copyErr
	}
	if closeErr != nil {
		return "", closeErr
	}
	if written > maximumDiscordAPKSize {
		return "", fmt.Errorf("Discord %d %s is larger than %d MiB", code, split, maximumDiscordAPKSize>>20)
	}
	return destination, nil
}

func apksignerPath() (string, error) {
	if path, err := exec.LookPath("apksigner"); err == nil {
		return path, nil
	}
	for _, root := range []string{os.Getenv("ANDROID_HOME"), os.Getenv("ANDROID_SDK_ROOT"), filepath.Join(os.Getenv("HOME"), "Library", "Android", "sdk")} {
		if root == "" {
			continue
		}
		matches, _ := filepath.Glob(filepath.Join(root, "build-tools", "*", "apksigner"))
		if len(matches) > 0 {
			sort.Strings(matches)
			return matches[len(matches)-1], nil
		}
	}
	return "", errors.New("apksigner was not found; install Android build-tools or put apksigner on PATH")
}

func verifyDiscordAPK(apksigner string, apk string) error {
	printed, err := exec.Command(apksigner, "verify", "--print-certs", apk).CombinedOutput()
	if err != nil {
		return fmt.Errorf("%s does not verify: %w\n%s", filepath.Base(apk), err, printed)
	}
	for _, line := range strings.Split(string(printed), "\n") {
		if !strings.HasPrefix(line, "Signer #1 certificate SHA-256 digest:") {
			continue
		}
		if strings.TrimSpace(strings.TrimPrefix(line, "Signer #1 certificate SHA-256 digest:")) == discordAPKCertificate {
			return nil
		}
		return fmt.Errorf("%s is not signed by Discord: %s", filepath.Base(apk), strings.TrimSpace(line))
	}
	return fmt.Errorf("%s has no signer certificate", filepath.Base(apk))
}

func downloadDiscordAPKs(channel string, directory string) (int, []string, error) {
	code, err := discordVersionCode(channel)
	if err != nil {
		return 0, nil, err
	}
	apksigner, err := apksignerPath()
	if err != nil {
		return 0, nil, err
	}
	var apks []string
	for _, split := range discordAPKSplits {
		apk, err := downloadDiscordSplit(code, split, directory)
		if err != nil {
			return 0, nil, err
		}
		if err := verifyDiscordAPK(apksigner, apk); err != nil {
			return 0, nil, err
		}
		apks = append(apks, apk)
	}
	return code, apks, nil
}
