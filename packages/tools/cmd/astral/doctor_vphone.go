package main

import (
	"errors"
	"os"
	"path"
	"path/filepath"
	"strings"
)

func vphoneMachineFile(name string) string {
	return filepath.Join(vphoneHome(), "machines", "vphone", name)
}

func checkVphonePreset() (string, error) {
	preset, err := os.ReadFile(vphoneMachineFile("PatchPreset.plist"))
	if err != nil {
		return "", errors.New("no vphone machine; run astral vphone create")
	}
	if strings.Contains(string(preset), adhocAdmission) {
		return "", errors.New("the machine keeps " + adhocAdmission + ", so ElleKit kills ad-hoc signed binaries")
	}
	selection, _ := os.ReadFile(vphoneMachineFile("PatchSelection.plist"))
	if strings.Contains(string(selection), "standard-adhoc") {
		return "standard-adhoc", nil
	}
	return "admission precheck off", nil
}

func vphoneJailbreakRoot() (vphoneClient, string, error) {
	client, err := newVphoneClient()
	if err != nil {
		return client, "", err
	}
	root, err := jailbreakRoot(client)
	return client, root, err
}

func checkVphoneElleKit() (string, error) {
	client, root, err := vphoneJailbreakRoot()
	if err != nil {
		return "", err
	}
	if !client.exists(path.Join(root, "usr", "lib"), "TweakLoader.dylib") {
		return "", errors.New("ElleKit is not installed")
	}
	status, err := vphoneRun("vphone", "awk '/^Package: ellekit$/,/^$/' "+shellQuote(path.Join(root, "var", "lib", "dpkg", "status")), false)
	if err != nil {
		return "", err
	}
	if !strings.Contains(status, "Dhinak") {
		return "", errors.New("ElleKit is not OwnGoal's build; the roothide repo's crashes in libinjector")
	}
	for _, line := range strings.Split(status, "\n") {
		if version, found := strings.CutPrefix(line, "Version: "); found {
			return "OwnGoal " + strings.TrimSpace(version), nil
		}
	}
	return "OwnGoal", nil
}

func checkVphoneTweak() (string, error) {
	client, root, err := vphoneJailbreakRoot()
	if err != nil {
		return "", err
	}
	tweaks := path.Join(root, "usr", "lib", "TweakInject")
	if !client.exists(tweaks, "astral.dylib") || !client.exists(tweaks, "astral.plist") {
		return "", errors.New("astral is not in TweakInject")
	}
	return "TweakInject/astral.dylib", nil
}

func checkVphoneDiscord() (string, error) {
	client, err := newVphoneClient()
	if err != nil {
		return "", err
	}
	var info struct {
		BundlePath string `json:"bundle_path"`
		Version    string `json:"version"`
	}
	if err := client.call("apps.info", map[string]any{"bundle_id": discordBundleID}, &info); err != nil {
		return "", errors.New("Discord is not installed")
	}
	if client.exists(path.Join(info.BundlePath, "Frameworks"), "astral.dylib") {
		return "", errors.New("Discord " + info.Version + " has astral built in, so the tweak would load twice")
	}
	return "stock Discord " + info.Version, nil
}
