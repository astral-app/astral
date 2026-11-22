package main

import (
	"archive/zip"
	"debug/macho"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"

	"howett.net/plist"
)

type probeResult struct {
	OpsArrived    uint64            `json:"opsArrived"`
	HostCall      uint64            `json:"hostCall"`
	PropertyCount int               `json:"propertyCount"`
	Resolved      int               `json:"resolved"`
	Missing       *string           `json:"missing"`
	Properties    map[string]uint16 `json:"properties"`
}

var errNoPhoenix = errors.New("this Discord build ships no Phoenix engine")

type compatResult struct {
	Phoenix     bool        `json:"phoenix"`
	Channel     string      `json:"channel"`
	Version     string      `json:"version"`
	Build       string      `json:"build"`
	PhoenixUUID string      `json:"phoenixUUID"`
	Probe       probeResult `json:"probe"`
	Supported   bool        `json:"supported"`
}

func extractPhoenix(ipa string, work string) (string, string, string, error) {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return "", "", "", err
	}
	defer archive.Close()
	app, err := appDirectory(archive)
	if err != nil {
		return "", "", "", err
	}
	info, err := readZipEntry(archive, app+"Info.plist")
	if err != nil {
		return "", "", "", err
	}
	var bundle struct {
		Version string `plist:"CFBundleShortVersionString"`
		Build   string `plist:"CFBundleVersion"`
	}
	if _, err := plist.Unmarshal(info, &bundle); err != nil {
		return "", "", "", err
	}
	binary, err := readZipEntry(archive, app+"Frameworks/PhoenixFatClient.framework/PhoenixFatClient")
	if err != nil {
		return "", bundle.Version, bundle.Build, errNoPhoenix
	}
	path := filepath.Join(work, "PhoenixFatClient")
	return path, bundle.Version, bundle.Build, os.WriteFile(path, binary, 0o644)
}

func imageUUID(path string) (string, error) {
	file, err := macho.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	const loadUUID = 0x1b
	for _, load := range file.Loads {
		raw := load.Raw()
		if len(raw) >= 24 && file.ByteOrder.Uint32(raw[0:4]) == loadUUID {
			text := hex.EncodeToString(raw[8:24])
			return text[0:8] + "-" + text[8:12] + "-" + text[12:16] + "-" + text[16:20] + "-" + text[20:32], nil
		}
	}
	return "", errors.New("PhoenixFatClient has no LC_UUID")
}

func buildProbe(work string) (string, error) {
	sources := repoPath("packages", "loader-ios")
	binary := filepath.Join(work, "compat-probe")
	return binary, run(sources, "cc", "-O1", "-Wall", "-Werror", "-o", binary, "tests/support/compat-probe.c", "sources/PhoenixLocate.c", "sources/PhoenixProperties.c")
}

func runProbe(probe string, phoenix string) (probeResult, error) {
	var result probeResult
	command := exec.Command(probe, phoenix)
	command.Stderr = os.Stderr
	text, err := command.Output()
	if len(text) == 0 {
		return result, fmt.Errorf("compat probe produced no output: %w", err)
	}
	return result, json.Unmarshal(text, &result)
}

func readBaseline(path string) *compatResult {
	if path == "" {
		return nil
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var baseline compatResult
	if json.Unmarshal(data, &baseline) != nil {
		return nil
	}
	return &baseline
}

func compatReport(result compatResult, baseline *compatResult) string {
	var report strings.Builder
	status := "supported"
	if !result.Supported {
		status = "**not supported**"
	}
	fmt.Fprintf(&report, "## astral on Discord %s (%s, %s)\n\n", result.Version, result.Build, result.Channel)
	fmt.Fprintf(&report, "Phoenix `%s` is %s.\n\n", result.PhoenixUUID, status)
	fmt.Fprintf(&report, "| Check | Result |\n| --- | --- |\n")
	fmt.Fprintf(&report, "| `opsArrived` | `0x%x` |\n", result.Probe.OpsArrived)
	fmt.Fprintf(&report, "| Host-call entry | `0x%x` |\n", result.Probe.HostCall)
	missing := "none"
	if result.Probe.Missing != nil {
		missing = "`" + *result.Probe.Missing + "`"
	}
	fmt.Fprintf(&report, "| View properties | %d in Phoenix, %d resolved, missing %s |\n", result.Probe.PropertyCount, result.Probe.Resolved, missing)
	if baseline == nil {
		return report.String()
	}
	var changes []string
	for name, id := range result.Probe.Properties {
		if before, found := baseline.Probe.Properties[name]; found && before != id {
			changes = append(changes, fmt.Sprintf("`%s` %d → %d", name, before, id))
		}
	}
	sort.Strings(changes)
	if len(changes) > 0 {
		fmt.Fprintf(&report, "\nProperty IDs that moved since Discord %s (%s), resolved at launch:\n\n- %s\n", baseline.Version, baseline.Build, strings.Join(changes, "\n- "))
	} else if baseline.PhoenixUUID != result.PhoenixUUID {
		fmt.Fprintf(&report, "\nNo property IDs moved since Discord %s (%s).\n", baseline.Version, baseline.Build)
	}
	return report.String()
}

func runCompat(args []string) error {
	flags := newFlags("compat")
	ipa := flags.String("ipa", "", "decrypted Discord IPA")
	channel := flags.String("channel", "stable", "release channel the IPA came from")
	baselinePath := flags.String("baseline", "", "previous compat.json to compare with")
	resultsPath := flags.String("results", "compat.json", "where to write the results")
	reportPath := flags.String("report", "", "where to write a Markdown report")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *ipa == "" {
		return errors.New("usage: astral compat --ipa <decrypted Discord.ipa> [--channel stable] [--baseline compat.json] [--results compat.json] [--report report.md]")
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	phoenix, version, build, err := extractPhoenix(*ipa, work)
	if errors.Is(err, errNoPhoenix) {
		return writeNoPhoenix(compatResult{Channel: *channel, Version: version, Build: build}, *resultsPath, *reportPath)
	}
	if err != nil {
		return err
	}
	uuid, err := imageUUID(phoenix)
	if err != nil {
		return err
	}
	probe, err := buildProbe(work)
	if err != nil {
		return err
	}
	probed, err := runProbe(probe, phoenix)
	if err != nil {
		return err
	}
	result := compatResult{true, *channel, version, build, uuid, probed, probed.OpsArrived != 0 && probed.HostCall != 0 && probed.Missing == nil}
	encoded, err := json.MarshalIndent(result, "", "\t")
	if err != nil {
		return err
	}
	if err := os.WriteFile(*resultsPath, append(encoded, '\n'), 0o644); err != nil {
		return err
	}
	report := compatReport(result, readBaseline(*baselinePath))
	if *reportPath != "" {
		if err := os.WriteFile(*reportPath, []byte(report), 0o644); err != nil {
			return err
		}
	}
	fmt.Print(report)
	if !result.Supported {
		return errors.New("this Discord build is not supported")
	}
	return nil
}

func writeNoPhoenix(result compatResult, resultsPath string, reportPath string) error {
	encoded, err := json.MarshalIndent(result, "", "\t")
	if err != nil {
		return err
	}
	if err := os.WriteFile(resultsPath, append(encoded, '\n'), 0o644); err != nil {
		return err
	}
	report := fmt.Sprintf("## astral on Discord %s (%s, %s)\n\nThis build ships no Phoenix engine, so astral has nothing to run on.\n", result.Version, result.Build, result.Channel)
	if reportPath != "" {
		if err := os.WriteFile(reportPath, []byte(report), 0o644); err != nil {
			return err
		}
	}
	fmt.Print(report)
	return errNoPhoenix
}
