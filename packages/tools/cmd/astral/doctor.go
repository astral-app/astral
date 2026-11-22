package main

import (
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"syscall"
	"time"
)

type severity int

const (
	advisory severity = iota
	required
)

type diagnosis struct {
	name   string
	level  severity
	check  func() (string, error)
	remedy string
}

type pinnedTool struct {
	binary  string
	pattern string
}

var pinnedTools = []pinnedTool{
	{"clang-format", `clang-format==([0-9.]+)`},
	{"stylua", `StyLua/releases/download/v([0-9.]+)`},
	{"taplo", `taplo/releases/download/([0-9.]+)/`},
	{"lua-language-server", `lua-language-server/releases/download/([0-9.]+)/`},
}

const minimumFreeGigabytes = 15

func runDoctor(args []string) error {
	flags := newFlags("doctor")
	if err := flags.Parse(args); err != nil {
		return err
	}
	pins, err := ciPins()
	if err != nil {
		return err
	}
	failed := 0
	for _, item := range diagnoses(pins) {
		detail, err := item.check()
		switch {
		case err == nil:
			fmt.Printf("ok    %-22s %s\n", item.name, detail)
		case item.level == required:
			failed++
			fmt.Printf("fail  %-22s %s; %s\n", item.name, err, item.remedy)
		default:
			fmt.Printf("warn  %-22s %s; %s\n", item.name, err, item.remedy)
		}
	}
	if failed > 0 {
		return fmt.Errorf("%d required checks failed", failed)
	}
	return nil
}

func diagnoses(pins map[string]string) []diagnosis {
	items := []diagnosis{
		{"git hooks", required, checkHooks, "run: git config core.hooksPath .githooks"},
		{"vendored sources", required, checkVendor, "run: git submodule update --init"},
		{"plugin signing key", required, checkFile(repoPath("keys", "plugin-signing.pub")), "restore keys/plugin-signing.pub from git"},
		{"clang", required, checkTool("clang", "--version"), "install the Xcode command line tools"},
		{"luacheck", required, checkTool("luacheck", "--version"), "run: brew install luacheck"},
	}
	for _, tool := range pinnedTools {
		items = append(items, diagnosis{tool.binary, required, checkPinned(tool.binary, pins[tool.binary]),
			fmt.Sprintf("install %s %s to match CI", tool.binary, pins[tool.binary])})
	}
	return append(items,
		diagnosis{"theos", advisory, checkTheos, "run: astral prepare-theos --theos ~/theos"},
		diagnosis{"gnu make", advisory, checkFile("/opt/homebrew/opt/make/libexec/gnubin/make"), "run: brew install make"},
		diagnosis{"ldid", advisory, checkTool("ldid"), "run: brew install ldid"},
		diagnosis{"android ndk", advisory, checkNDK, "install an NDK with Android Studio or set ANDROID_NDK_HOME"},
		diagnosis{"vphone-cli", advisory, checkTool("vphone-cli", "--help"), "install vphone-cli for astral vphone"},
		diagnosis{"ideviceinstaller", advisory, checkTool("ideviceinstaller", "--help"), "run: brew install ideviceinstaller"},
		diagnosis{"vphone api", advisory, checkVphoneAPI, "run: astral vphone boot"},
		diagnosis{"one vm at a time", advisory, checkSingleVM, "stop the AVD or the vphone; the Mac lacks memory for both"},
		diagnosis{"free disk", advisory, checkDisk, fmt.Sprintf("free at least %d GB", minimumFreeGigabytes)},
	)
}

func ciPins() (map[string]string, error) {
	workflow, err := os.ReadFile(repoPath(".github", "workflows", "ci.yml"))
	if err != nil {
		return nil, err
	}
	return pinsFrom(string(workflow)), nil
}

func pinsFrom(workflow string) map[string]string {
	pins := map[string]string{}
	for _, tool := range pinnedTools {
		if match := regexp.MustCompile(tool.pattern).FindStringSubmatch(workflow); match != nil {
			pins[tool.binary] = match[1]
		}
	}
	return pins
}

func toolOutput(binary string, args ...string) ([]string, error) {
	if _, err := exec.LookPath(binary); err != nil {
		return nil, errors.New("not on PATH")
	}
	result, err := exec.Command(binary, args...).CombinedOutput()
	text := strings.TrimSpace(string(result))
	if err != nil && text == "" {
		return nil, err
	}
	return strings.Split(text, "\n"), nil
}

func toolVersion(binary string) (string, error) {
	lines, err := toolOutput(binary, "--version")
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(lines[len(lines)-1]), nil
}

func checkTool(binary string, args ...string) func() (string, error) {
	return func() (string, error) {
		lines, err := toolOutput(binary, args...)
		if err != nil {
			return "", err
		}
		first := strings.TrimSpace(lines[0])
		if len(first) > 60 {
			first = first[:60]
		}
		return first, nil
	}
}

func checkPinned(binary string, pinned string) func() (string, error) {
	return func() (string, error) {
		version, err := toolVersion(binary)
		if err != nil {
			return "", err
		}
		switch {
		case pinned == "" || versionMatches(version, pinned):
			return version, nil
		case versionMatches(version, minorOf(pinned)+`.[0-9]+`):
			return fmt.Sprintf("%s (CI pins %s; same minor)", version, pinned), nil
		}
		return "", fmt.Errorf("found %s, CI pins %s", version, pinned)
	}
}

func versionMatches(output string, pinned string) bool {
	pattern := regexp.QuoteMeta(pinned)
	if strings.HasSuffix(pinned, `.[0-9]+`) {
		pattern = regexp.QuoteMeta(strings.TrimSuffix(pinned, `.[0-9]+`)) + `\.[0-9]+`
	}
	return regexp.MustCompile(`(^|[^0-9.])` + pattern + `($|[^0-9.])`).MatchString(output)
}

func minorOf(version string) string {
	parts := strings.SplitN(version, ".", 3)
	if len(parts) < 2 {
		return version
	}
	return parts[0] + "." + parts[1]
}

func checkFile(path string) func() (string, error) {
	return func() (string, error) {
		if _, err := os.Stat(path); err != nil {
			return "", errors.New("missing " + path)
		}
		return path, nil
	}
}

func checkHooks() (string, error) {
	hooks, err := output(repoPath(), "git", "config", "core.hooksPath")
	if strings.TrimSpace(hooks) != ".githooks" {
		if err == nil {
			err = fmt.Errorf("hooks path is %q", strings.TrimSpace(hooks))
		}
		return "", err
	}
	return ".githooks", nil
}

func checkVendor() (string, error) {
	for _, file := range []string{"lua/lua.h", "json/single_include/nlohmann/json.hpp", "tomlplusplus/include/toml++/toml.hpp", "monocypher/src/monocypher.c"} {
		if _, err := os.Stat(repoPath("vendor", file)); err != nil {
			return "", errors.New("missing vendor/" + file)
		}
	}
	return "lua, json, tomlplusplus, monocypher", nil
}

func checkTheos() (string, error) {
	theos := os.Getenv("THEOS")
	if theos == "" {
		home, _ := os.UserHomeDir()
		theos = filepath.Join(home, "theos")
	}
	sdks, _ := filepath.Glob(filepath.Join(theos, "sdks", "iPhoneOS*.sdk"))
	if _, err := os.Stat(filepath.Join(theos, "makefiles", "common.mk")); err != nil || len(sdks) == 0 {
		return "", errors.New("no Theos with an iPhoneOS SDK at " + theos)
	}
	return fmt.Sprintf("%s (%d SDKs)", theos, len(sdks)), nil
}

func checkNDK() (string, error) {
	toolchain, err := ndkToolchain("")
	if err != nil {
		return "", err
	}
	return filepath.Dir(filepath.Dir(filepath.Dir(filepath.Dir(filepath.Dir(toolchain))))), nil
}

func checkVphoneAPI() (string, error) {
	client, err := newVphoneClient()
	if err != nil {
		return "", err
	}
	connection, err := net.DialTimeout("tcp", client.address, time.Second)
	if err != nil {
		return "", errors.New("nothing listening on " + client.address)
	}
	connection.Close()
	return client.address, nil
}

func checkSingleVM() (string, error) {
	avd := processRunning("qemu-system")
	vphone := processRunning("vphone-cli")
	if avd && vphone {
		return "", errors.New("the AVD and the vphone are both running")
	}
	switch {
	case avd:
		return "AVD running", nil
	case vphone:
		return "vphone running", nil
	}
	return "neither running", nil
}

func processRunning(name string) bool {
	return exec.Command("pgrep", "-f", name).Run() == nil
}

func checkDisk() (string, error) {
	var stats syscall.Statfs_t
	if err := syscall.Statfs(repoPath(), &stats); err != nil {
		return "", err
	}
	free := stats.Bavail * uint64(stats.Bsize) >> 30
	if free < minimumFreeGigabytes {
		return "", fmt.Errorf("%d GB free", free)
	}
	return fmt.Sprintf("%d GB free", free), nil
}
