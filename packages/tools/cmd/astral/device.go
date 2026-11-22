package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
)

type pilot struct {
	binary string
	device string
}

type shellResult struct {
	ExitCode int    `json:"exitCode"`
	Output   string `json:"output"`
}

type snapshotNode struct {
	Label           string `json:"label"`
	ActivationPoint struct {
		X float64 `json:"x"`
		Y float64 `json:"y"`
	} `json:"activationPoint"`
}

func newPilot(device string) (pilot, error) {
	if err := requireDevice(device); err != nil {
		return pilot{}, err
	}
	binary := os.Getenv("JB_PILOT")
	if binary == "" {
		home, _ := os.UserHomeDir()
		binary = filepath.Join(home, "go", "bin", "jb-p1lot")
	}
	return pilot{binary, device}, nil
}

func (p pilot) call(args ...string) (string, error) {
	return output(repoPath(), p.binary, append([]string{args[0], "--device", p.device}, args[1:]...)...)
}

func (p pilot) shell(command string) (string, error) {
	raw, err := p.call("shell_exec", "--command", command)
	if err != nil {
		return "", err
	}
	var result shellResult
	if err := json.Unmarshal([]byte(raw), &result); err != nil {
		return "", fmt.Errorf("unexpected jb-p1lot output: %s", strings.TrimSpace(raw))
	}
	return result.Output, nil
}

func (p pilot) push(local string, remote string) error {
	_, err := p.call("file_transfer", "--direction", "host_to_device", "--source", local, "--destination", remote)
	return err
}

func (p pilot) pull(remote string, local string) error {
	_, err := p.call("file_transfer", "--direction", "device_to_host", "--source", remote, "--destination", local)
	return err
}

func (p pilot) tap(x int, y int, settle time.Duration) {
	p.call("ui_action", "--action", "tap", "--x", fmt.Sprint(x), "--y", fmt.Sprint(y))
	time.Sleep(settle)
}

func (p pilot) labels() ([]snapshotNode, error) {
	raw, err := p.call("ui_snapshot")
	if err != nil {
		return nil, err
	}
	var snapshot struct {
		Nodes []snapshotNode `json:"nodes"`
	}
	if err := json.Unmarshal([]byte(raw), &snapshot); err != nil {
		return nil, err
	}
	return snapshot.Nodes, nil
}

func (p pilot) stateDirectory() (string, error) {
	listing, err := p.shell("ls -d /var/mobile/Containers/Data/Application/*/Documents/astral | head -1")
	if err != nil {
		return "", err
	}
	directory := strings.TrimSpace(listing)
	if directory == "" {
		return "", errors.New("no astral state directory on the device")
	}
	return directory, nil
}

func newestDeb() (string, error) {
	debs, err := filepath.Glob(repoPath("packages", "ios", "packages", "*.deb"))
	if err != nil {
		return "", err
	}
	if len(debs) == 0 {
		return "", errors.New("no astral .deb; build packages/ios first")
	}
	sort.Slice(debs, func(left, right int) bool {
		leftInfo, _ := os.Stat(debs[left])
		rightInfo, _ := os.Stat(debs[right])
		return leftInfo.ModTime().Before(rightInfo.ModTime())
	})
	return debs[len(debs)-1], nil
}

func buildPackage() error {
	theos := os.Getenv("THEOS")
	if theos == "" {
		home, _ := os.UserHomeDir()
		theos = filepath.Join(home, "theos")
	}
	path := "/opt/homebrew/opt/make/libexec/gnubin:/opt/homebrew/bin:" + os.Getenv("PATH")
	return runWithEnv(repoPath("packages", "ios"), []string{"THEOS=" + theos, "PATH=" + path}, "make", "package", "FINALPACKAGE=1")
}

func runDeploy(args []string) error {
	flags := newFlags("deploy")
	device := deviceFlag(flags)
	if err := flags.Parse(args); err != nil {
		return err
	}
	p, err := newPilot(*device)
	if err != nil {
		return err
	}
	if err := buildPackage(); err != nil {
		return err
	}
	deb, err := newestDeb()
	if err != nil {
		return err
	}
	if err := p.push(deb, "/var/mobile/Documents/astral.deb"); err != nil {
		return err
	}
	if _, err := p.shell("dpkg -i /var/mobile/Documents/astral.deb >/dev/null 2>&1; killall -9 Discord 2>/dev/null; sleep 1; /var/jb/usr/bin/uiopen --bundleid com.hammerandchisel.discord"); err != nil {
		return err
	}
	fmt.Println("installed " + filepath.Base(deb))
	return nil
}

func labelAt(nodes []snapshotNode, label string) (snapshotNode, bool) {
	for index := len(nodes) - 1; index >= 0; index-- {
		if nodes[index].Label == label {
			return nodes[index], true
		}
	}
	return snapshotNode{}, false
}

func hasLabel(nodes []snapshotNode, pattern *regexp.Regexp) bool {
	for _, node := range nodes {
		if pattern.MatchString(node.Label) {
			return true
		}
	}
	return false
}

func printLabels(nodes []snapshotNode) {
	for _, node := range nodes {
		fmt.Printf("%s %d %d\n", strings.ReplaceAll(node.Label, " ", "_"), int(node.ActivationPoint.X), int(node.ActivationPoint.Y))
	}
}

func runOpenSettings(args []string) error {
	flags := newFlags("open-settings")
	device := deviceFlag(flags)
	if err := flags.Parse(args); err != nil {
		return err
	}
	target := "General"
	if flags.NArg() > 0 {
		target = strings.Join(flags.Args(), " ")
	}
	p, err := newPilot(*device)
	if err != nil {
		return err
	}
	status := regexp.MustCompile(`, (Idle|Online|Do Not Disturb|Invisible)$`)
	for attempt := 0; attempt < 6; attempt++ {
		nodes, err := p.labels()
		if err != nil {
			return err
		}
		if _, onSettings := labelAt(nodes, "Account Settings"); onSettings {
			if row, found := labelAt(nodes, target); found {
				p.tap(187, int(row.ActivationPoint.Y), 3*time.Second)
				after, err := p.labels()
				if err != nil {
					return err
				}
				printLabels(after)
				return nil
			}
		}
		if button, found := labelAt(nodes, "Settings"); found {
			p.tap(int(button.ActivationPoint.X), int(button.ActivationPoint.Y), 3*time.Second)
			continue
		}
		if hasLabel(nodes, status) {
			for _, node := range nodes {
				if status.MatchString(node.Label) {
					p.tap(int(node.ActivationPoint.X), int(node.ActivationPoint.Y), 3*time.Second)
					break
				}
			}
			continue
		}
		p.tap(28, 48, 2*time.Second)
	}
	return errors.New("could not reach the " + target + " row")
}

func runWireDump(args []string) error {
	flags := newFlags("wire-dump")
	device := deviceFlag(flags)
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 1 {
		return errors.New("usage: astral wire-dump [--device <udid>] <output directory>")
	}
	destination := flags.Arg(0)
	p, err := newPilot(*device)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(destination, 0o755); err != nil {
		return err
	}
	tool := filepath.Join(destination, ".wire-dump")
	support := repoPath("packages", "ios", "tests", "support")
	if err := run(repoPath(), "clang", "-O1", "-o", tool, filepath.Join(support, "wire-dump.c"), filepath.Join(support, "wire-parser.c")); err != nil {
		return err
	}
	state, err := p.stateDirectory()
	if err != nil {
		return err
	}
	listing, err := p.shell("ls " + state + "/wire")
	if err != nil {
		return err
	}
	var batches []string
	for _, name := range strings.Fields(listing) {
		local := filepath.Join(destination, name)
		if err := p.pull(state+"/wire/"+name, local); err != nil {
			return err
		}
		batches = append(batches, local)
	}
	if len(batches) == 0 {
		return errors.New("no captured batches; create capture-wire in astral's state directory and relaunch")
	}
	dump, err := output(repoPath(), tool, batches...)
	if err != nil {
		return err
	}
	jsonl := filepath.Join(destination, "wire.jsonl")
	if err := os.WriteFile(jsonl, []byte(dump), 0o644); err != nil {
		return err
	}
	fmt.Printf("%d batches in %s\n", strings.Count(dump, "\n"), jsonl)
	return nil
}
