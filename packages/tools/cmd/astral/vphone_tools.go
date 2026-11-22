package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"

	"howett.net/plist"
)

const (
	vphoneCaskPresets  = "/opt/homebrew/Caskroom/vphone-cli"
	adhocPresetName    = "standard-adhoc.plist"
	adhocAdmission     = "txm-exp-precheck_admission"
	minimumCreateSpace = 50
)

func vphoneHome() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".vphone")
}

func bundledStandardPreset() (string, error) {
	matches, err := filepath.Glob(filepath.Join(vphoneCaskPresets, "*", "VPhone.bundle", "Contents", "Resources", "patches_presets", "standard.plist"))
	if err != nil || len(matches) == 0 {
		return "", errors.New("no vphone-cli install found; run brew install --cask vphone-cli")
	}
	sort.Strings(matches)
	return matches[len(matches)-1], nil
}

func writeAdhocPreset() (string, error) {
	standard, err := bundledStandardPreset()
	if err != nil {
		return "", err
	}
	data, err := os.ReadFile(standard)
	if err != nil {
		return "", err
	}
	encoded, err := adhocPreset(data)
	if err != nil {
		return "", err
	}
	target := filepath.Join(vphoneHome(), "presets", adhocPresetName)
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		return "", err
	}
	return target, os.WriteFile(target, encoded, 0o644)
}

func adhocPreset(data []byte) ([]byte, error) {
	var preset map[string]any
	if _, err := plist.Unmarshal(data, &preset); err != nil {
		return nil, err
	}
	preset["Identifier"] = "standard-adhoc"
	preset["Title"] = "Standard with ad-hoc admission"
	if selection, ok := preset["Selection"].(map[string]any); ok {
		if patches, ok := selection["Patches"].([]any); ok {
			kept := []any{}
			for _, patch := range patches {
				if patch != adhocAdmission {
					kept = append(kept, patch)
				}
			}
			selection["Patches"] = kept
		}
	}
	return plist.Marshal(preset, plist.XMLFormat)
}

func freeGigabytes(directory string) int {
	output, err := exec.Command("df", "-g", directory).Output()
	if err != nil {
		return 0
	}
	lines := strings.Split(strings.TrimSpace(string(output)), "\n")
	fields := strings.Fields(lines[len(lines)-1])
	if len(fields) < 4 {
		return 0
	}
	free, _ := strconv.Atoi(fields[3])
	return free
}

func runStreaming(name string, args []string, onLine func(string)) error {
	command := exec.Command(name, args...)
	stdout, err := command.StdoutPipe()
	if err != nil {
		return err
	}
	command.Stderr = command.Stdout
	if err := command.Start(); err != nil {
		return err
	}
	scanner := bufio.NewScanner(stdout)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	for scanner.Scan() {
		line := scanner.Text()
		fmt.Println(line)
		onLine(line)
	}
	return command.Wait()
}

func clearIPSWCache() {
	matches, _ := filepath.Glob(filepath.Join(vphoneHome(), "ipsws", "*"))
	for _, match := range matches {
		exec.Command("sudo", "-n", "rm", "-rf", match).Run()
	}
}

func resumeTemplateBuild() (bool, error) {
	builds, _ := filepath.Glob(filepath.Join(vphoneHome(), "machines", ".templates", ".building-*"))
	if len(builds) == 0 {
		return false, nil
	}
	build := builds[len(builds)-1]
	entries, err := os.ReadDir(build)
	if err != nil || len(entries) == 0 {
		return false, err
	}
	fmt.Println("resuming the template build " + filepath.Base(build))
	if err := run(repoPath(), "sudo", "-n", "vphone-cli", "cfw", "install", "--library-root", build, entries[0].Name()); err != nil {
		return false, err
	}
	return true, run(repoPath(), "vphone-cli", "vm", "template", "setup", filepath.Base(build))
}

func createVM(name string, build string, preset string, disk int) error {
	args := []string{"-n", "vphone-cli", "vm", "create", name, "--iphone-build", build, "--device", "iPhone17,3", "--preset", preset,
		"--bootstrap", "roothide", "--unlock-at-startup", "on", "--disk-size", strconv.Itoa(disk)}
	return runStreaming("sudo", args, func(line string) {
		if strings.Contains(line, "Firmware prepared") {
			clearIPSWCache()
		}
	})
}

func runVphoneCreate(args []string) error {
	flags := newFlags("vphone create")
	name := flags.String("name", "vphone", "VM name")
	build := flags.String("build", "24B156", "iOS build to install")
	disk := flags.Int("disk", 48, "disk size in GB")
	ipa := flags.String("ipa", "", "decrypted Discord IPA to install and deploy astral into")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if exec.Command("sudo", "-n", "true").Run() != nil {
		return errors.New("vphone-cli needs sudo without a password prompt for vm create")
	}
	if free := freeGigabytes(vphoneHome()); free < minimumCreateSpace {
		fmt.Printf("warning: %d GB free; the firmware install needs %d GB (the IPSW cache is cleared once firmware is prepared)\n", free, minimumCreateSpace)
	}
	preset, err := writeAdhocPreset()
	if err != nil {
		return err
	}
	if err := createVM(*name, *build, preset, *disk); err != nil {
		resumed, resumeErr := resumeTemplateBuild()
		if resumeErr != nil || !resumed {
			return fmt.Errorf("vm create failed: %w (free space: %d GB)", err, freeGigabytes(vphoneHome()))
		}
		if err := createVM(*name, *build, preset, *disk); err != nil {
			return err
		}
	}
	if err := bootVphone(*name); err != nil {
		return err
	}
	if err := runVphoneSetup([]string{"--name", *name}); err != nil {
		return err
	}
	if *ipa == "" {
		fmt.Println("created " + *name + "; install Discord with astral vphone discord --ipa <IPA>, then astral vphone deploy")
		return nil
	}
	client, err := newVphoneClient()
	if err != nil {
		return err
	}
	if err := vphoneInstallDiscord(client, []string{"--ipa", *ipa, "--name", *name}); err != nil {
		return err
	}
	return vphoneTweakDeploy(client)
}

func snapshotVM(name string, action string, snapshot string) error {
	if action == "list" {
		return run(repoPath(), "vphone-cli", "vm", "snapshot", "list", name)
	}
	if snapshot == "" {
		return errors.New("usage: astral vphone snapshot <save|revert|list> [name]")
	}
	if err := run(repoPath(), "vphone-cli", "vm", "stop", name, "--timeout", "60"); err != nil {
		return err
	}
	switch action {
	case "save":
		if err := run(repoPath(), "vphone-cli", "vm", "snapshot", "create", name, snapshot); err != nil {
			return err
		}
	case "revert":
		command := exec.Command("vphone-cli", "vm", "snapshot", "revert", name, snapshot)
		command.Stdin = strings.NewReader("y\n")
		command.Stdout, command.Stderr = os.Stdout, os.Stderr
		if err := command.Run(); err != nil {
			return err
		}
	default:
		return errors.New("usage: astral vphone snapshot <save|revert|list> [name]")
	}
	return bootVphone(name)
}

func runVphoneSnapshot(args []string) error {
	flags := newFlags("vphone snapshot")
	name := flags.String("name", "vphone", "VM name")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() == 0 {
		return errors.New("usage: astral vphone snapshot <save|revert|list> [snapshot]")
	}
	return snapshotVM(*name, flags.Arg(0), flags.Arg(1))
}

func watchedSources() []string {
	return []string{repoPath("packages", "ios", "sources"), repoPath("packages", "core"), repoPath("packages", "addons", "themes"), repoPath("packages", "addons", "plugins")}
}

func newestChange(roots []string) time.Time {
	var newest time.Time
	for _, root := range roots {
		filepath.WalkDir(root, func(file string, entry fs.DirEntry, err error) error {
			if err != nil {
				return nil
			}
			if entry.IsDir() && (entry.Name() == "build" || strings.HasPrefix(entry.Name(), ".")) {
				return filepath.SkipDir
			}
			if info, err := entry.Info(); err == nil && info.ModTime().After(newest) {
				newest = info.ModTime()
			}
			return nil
		})
	}
	return newest
}

func runVphoneWatch(client vphoneClient) error {
	roots := watchedSources()
	last := newestChange(roots)
	fmt.Println("watching ios, core, themes and plugins; every save rebuilds and deploys")
	for {
		time.Sleep(500 * time.Millisecond)
		changed := newestChange(roots)
		if !changed.After(last) {
			continue
		}
		time.Sleep(300 * time.Millisecond)
		last = newestChange(roots)
		if err := vphoneTweakDeploy(client); err != nil {
			fmt.Println("deploy failed: " + err.Error())
		}
	}
}

const crashReports = "/var/mobile/Library/Logs/CrashReporter"

type crashImage struct {
	Name string `json:"name"`
	Base uint64 `json:"base"`
	Arch string `json:"arch"`
}

type crashFrame struct {
	ImageIndex  *int   `json:"imageIndex"`
	ImageOffset uint64 `json:"imageOffset"`
	Symbol      string `json:"symbol"`
}

type crashReport struct {
	Exception struct {
		Type    string `json:"type"`
		Signal  string `json:"signal"`
		Subtype string `json:"subtype"`
	} `json:"exception"`
	Termination struct {
		Namespace string `json:"namespace"`
		Indicator string `json:"indicator"`
	} `json:"termination"`
	UsedImages []crashImage `json:"usedImages"`
	Threads    []struct {
		Triggered bool         `json:"triggered"`
		Frames    []crashFrame `json:"frames"`
	} `json:"threads"`
}

func newestCrash(client vphoneClient, prefix string) (string, error) {
	var listing struct {
		Entries []guestEntry `json:"entries"`
	}
	if err := client.call("files.list", map[string]any{"path": crashReports, "limit": 1000}, &listing); err != nil {
		return "", err
	}
	var names []string
	for _, entry := range listing.Entries {
		if strings.HasPrefix(entry.Name, prefix+"-") && strings.HasSuffix(entry.Name, ".ips") {
			names = append(names, entry.Name)
		}
	}
	if len(names) == 0 {
		return "", errors.New("no " + prefix + " crash reports on the vphone")
	}
	sort.Strings(names)
	return names[len(names)-1], nil
}

func astralDSYM() string {
	for _, arch := range []string{"arm64e", "arm64"} {
		candidate := repoPath("packages", "ios", ".theos", "obj", arch, "astral.dylib.dSYM")
		if fileExists(candidate) {
			return candidate
		}
	}
	return ""
}

func symbolicate(image crashImage, offset uint64) string {
	dsym := astralDSYM()
	if dsym == "" {
		return ""
	}
	arch := image.Arch
	if arch == "" {
		arch = "arm64e"
	}
	output, err := exec.Command("atos", "-arch", arch, "-o", dsym, "-l", fmt.Sprintf("0x%x", image.Base), fmt.Sprintf("0x%x", image.Base+offset)).Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(output))
}

var ipsHeader = regexp.MustCompile(`^\{.*\}\n`)

func runVphoneCrashes(client vphoneClient, args []string) error {
	flags := flag.NewFlagSet("vphone crashes", flag.ContinueOnError)
	process := flags.String("process", "Discord", "process name the report starts with")
	if err := flags.Parse(args); err != nil {
		return err
	}
	name, err := newestCrash(client, *process)
	if err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	local := filepath.Join(work, name)
	if err := client.pull(path.Join(crashReports, name), local); err != nil {
		return err
	}
	raw, err := os.ReadFile(local)
	if err != nil {
		return err
	}
	body := ipsHeader.ReplaceAll(raw, nil)
	var report crashReport
	if err := json.Unmarshal(body, &report); err != nil {
		return fmt.Errorf("parse %s: %w", name, err)
	}
	fmt.Printf("%s\n%s %s %s; %s %s\n", name, report.Exception.Type, report.Exception.Signal, report.Exception.Subtype,
		report.Termination.Namespace, report.Termination.Indicator)
	for _, thread := range report.Threads {
		if !thread.Triggered {
			continue
		}
		for index, frame := range thread.Frames {
			if index >= 20 {
				break
			}
			image := crashImage{}
			if frame.ImageIndex != nil && *frame.ImageIndex < len(report.UsedImages) {
				image = report.UsedImages[*frame.ImageIndex]
			}
			symbol := frame.Symbol
			if image.Name == "astral.dylib" {
				if resolved := symbolicate(image, frame.ImageOffset); resolved != "" {
					symbol = resolved
				}
			}
			fmt.Printf("%2d  %-24s %s +0x%x\n", index, image.Name, symbol, frame.ImageOffset)
		}
	}
	return nil
}
