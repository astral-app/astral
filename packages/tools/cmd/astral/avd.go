package main

import (
	"encoding/xml"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"syscall"
	"time"
)

const avdUsage = `usage: astral avd <command>
  boot [--name <avd>]            start the emulator headless and wait until it is ready (refuses while the vphone runs)
  stop                           shut the emulator down
  status                         Discord version, launch client, module state and astral's dev status
  discord --apk <APK>...         install Discord's base APK and splits, keeping its data
  deploy                         build the astral module, install and enable it, relaunch Discord on Phoenix
  launch                         relaunch Discord and wait for Phoenix
  screenshot <file.png>          save the screen
  open <target>                  open a Discord link: user <id>, channel <guild> <channel> [message], dm [channel], settings [section] or a URL
  tap <text|id|label>            tap the first view whose text, resource id or content description matches
  settings [row]                 open Discord's Settings, then optionally an astral row (general, plugins, design…)
  logs [--since <seconds>]       astral's logcat lines`

const (
	discordPackage    = "com.discord"
	discordActivity   = "com.discord.nativeapp.MainActivity"
	vectorCLI         = "sh /data/adb/modules/zygisk_vector/cli"
	astralModuleID    = "app.astral.android"
	phoenixPreference = "/data/data/com.discord/shared_prefs/discord_fat_client.xml"
)

func runAVD(args []string) error {
	if len(args) == 0 {
		return errors.New(avdUsage)
	}
	switch args[0] {
	case "boot":
		return avdBoot(args[1:])
	case "stop":
		return avdShell("emu", "kill")
	case "status":
		return avdStatus()
	case "discord":
		return avdInstallDiscord(args[1:])
	case "deploy":
		return avdDeploy(args[1:])
	case "launch":
		return avdLaunch()
	case "screenshot":
		if len(args) != 2 {
			return errors.New(avdUsage)
		}
		return avdScreenshot(args[1])
	case "open":
		link, err := discordLink(args[1:])
		if err != nil {
			return err
		}
		return avdOpen(link)
	case "tap":
		if len(args) != 2 {
			return errors.New(avdUsage)
		}
		return avdTap(args[1])
	case "settings":
		row := ""
		if len(args) > 1 {
			row = args[1]
		}
		return avdSettings(row)
	case "logs":
		return avdLogs(args[1:])
	default:
		return errors.New(avdUsage)
	}
}

func androidSDK() string {
	if sdk := os.Getenv("ANDROID_HOME"); sdk != "" {
		return sdk
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Android", "sdk")
}

func adbBinary() string {
	candidate := filepath.Join(androidSDK(), "platform-tools", "adb")
	if fileExists(candidate) {
		return candidate
	}
	return "adb"
}

func avdSerial() string {
	return os.Getenv("ANDROID_SERIAL")
}

func avdShell(arguments ...string) error {
	return run(repoPath(), adbBinary(), adbArguments(avdSerial(), arguments...)...)
}

func avdOutput(arguments ...string) (string, error) {
	return output(repoPath(), adbBinary(), adbArguments(avdSerial(), arguments...)...)
}

func avdRoot(command string) (string, error) {
	return avdOutput("shell", "su", "0", "sh", "-c", command)
}

func vphoneRunning() bool {
	return exec.Command("pgrep", "-f", "vphone-cli boot").Run() == nil
}

func avdBooted() bool {
	value, err := avdOutput("shell", "getprop", "sys.boot_completed")
	return err == nil && strings.TrimSpace(value) == "1"
}

func defaultAVD() (string, error) {
	if name := os.Getenv("ASTRAL_AVD"); name != "" {
		return name, nil
	}
	listing, err := output(repoPath(), filepath.Join(androidSDK(), "emulator", "emulator"), "-list-avds")
	if err != nil {
		return "", err
	}
	names := strings.Fields(listing)
	if len(names) == 0 {
		return "", errors.New("no AVD exists; create one with the Android SDK first")
	}
	return names[0], nil
}

func avdBoot(args []string) error {
	flags := newFlags("avd boot")
	name := flags.String("name", "", "AVD name (default ASTRAL_AVD or the first AVD)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if vphoneRunning() {
		return errors.New("the vphone is running; stop it first (vphone-cli vm stop vphone): the Mac cannot run both")
	}
	if avdBooted() {
		fmt.Println("the emulator is already running")
		return avdPrepareScreen()
	}
	if *name == "" {
		found, err := defaultAVD()
		if err != nil {
			return err
		}
		*name = found
	}
	home, _ := os.UserHomeDir()
	logPath := filepath.Join(home, ".cache", "astral", "avd.log")
	if err := os.MkdirAll(filepath.Dir(logPath), 0o755); err != nil {
		return err
	}
	logFile, err := os.Create(logPath)
	if err != nil {
		return err
	}
	defer logFile.Close()
	emulator := exec.Command(filepath.Join(androidSDK(), "emulator", "emulator"), "-avd", *name, "-no-snapshot-save", "-no-boot-anim")
	emulator.Stdout = logFile
	emulator.Stderr = logFile
	emulator.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	if err := emulator.Start(); err != nil {
		return err
	}
	go emulator.Wait()
	deadline := time.Now().Add(4 * time.Minute)
	for !avdBooted() {
		if time.Now().After(deadline) {
			return fmt.Errorf("the emulator did not finish booting in 4 minutes; read %s", logPath)
		}
		time.Sleep(3 * time.Second)
	}
	avdShell("root")
	time.Sleep(2 * time.Second)
	if err := avdPrepareScreen(); err != nil {
		return err
	}
	fmt.Printf("%s booted; log in %s\n", *name, logPath)
	return nil
}

func avdPrepareScreen() error {
	for _, command := range [][]string{
		{"shell", "svc", "power", "stayon", "true"},
		{"shell", "settings", "put", "system", "screen_off_timeout", "2147483647"},
		{"shell", "input", "keyevent", "KEYCODE_WAKEUP"},
		{"shell", "wm", "dismiss-keyguard"},
	} {
		if err := avdShell(command...); err != nil {
			return err
		}
	}
	return nil
}

func avdInstallDiscord(args []string) error {
	flags := newFlags("avd discord")
	var apks androidAPKList
	flags.Var(&apks, "apk", "Discord base or split APK (repeat)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	inputs, err := androidAPKInputs(apks)
	if err != nil {
		return err
	}
	if len(inputs) == 0 {
		return errors.New("pass --apk for the base APK and each split")
	}
	return avdShell(append([]string{"install-multiple", "-r", "-d"}, inputs...)...)
}

func avdDeploy(args []string) error {
	if err := buildAndroidModule(args); err != nil {
		return err
	}
	module := filepath.Join(repoPath("packages", "android", "build", "astral"), "astral-module.apk")
	if err := avdShell("install", "-r", module); err != nil {
		return err
	}
	for _, command := range []string{
		vectorCLI + " modules enable " + astralModuleID,
		vectorCLI + " scope set " + astralModuleID + " " + discordPackage + "/0",
	} {
		if _, err := avdRoot(command); err != nil {
			return err
		}
	}
	return avdLaunch()
}

func avdLaunch() error {
	avdShell("shell", "am", "force-stop", discordPackage)
	time.Sleep(time.Second)
	if err := avdShell("shell", "monkey", "-p", discordPackage, "-c", "android.intent.category.LAUNCHER", "1"); err != nil {
		return err
	}
	deadline := time.Now().Add(40 * time.Second)
	for time.Now().Before(deadline) {
		time.Sleep(2 * time.Second)
		if strings.Contains(avdTopActivity(), discordActivity) {
			time.Sleep(15 * time.Second)
			fmt.Println("Discord runs on Phoenix")
			return nil
		}
	}
	return fmt.Errorf("Discord did not open on Phoenix; it shows %s", strings.TrimSpace(avdTopActivity()))
}

func avdTopActivity() string {
	listing, _ := avdOutput("shell", "dumpsys", "activity", "activities")
	for _, line := range strings.Split(listing, "\n") {
		if strings.Contains(line, "topResumedActivity") {
			return line
		}
	}
	return ""
}

func avdStatus() error {
	if !avdBooted() {
		return errors.New("the emulator is not running; run astral avd boot")
	}
	version, _ := avdOutput("shell", "dumpsys", "package", discordPackage)
	for _, line := range strings.Split(version, "\n") {
		if strings.Contains(line, "versionName=") {
			fmt.Println("Discord", strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(line), "versionName=")))
			break
		}
	}
	client, _ := avdRoot("cat " + phoenixPreference)
	launch := regexp.MustCompile(`name="launch_client">([a-z]+)<`).FindStringSubmatch(client)
	if len(launch) == 2 {
		fmt.Println("launch client:", launch[1])
	}
	modules, _ := avdRoot(vectorCLI + " modules ls")
	for _, line := range strings.Split(modules, "\n") {
		if strings.Contains(line, astralModuleID) {
			fmt.Println("module:", strings.Join(strings.Fields(line), " "))
		}
	}
	fmt.Println("screen:", strings.TrimSpace(avdTopActivity()))
	return nil
}

func avdScreenshot(path string) error {
	data, err := avdOutput("exec-out", "screencap", "-p")
	if err != nil {
		return err
	}
	return os.WriteFile(path, []byte(data), 0o644)
}

type uiNode struct {
	Text        string   `xml:"text,attr"`
	ResourceID  string   `xml:"resource-id,attr"`
	Description string   `xml:"content-desc,attr"`
	Bounds      string   `xml:"bounds,attr"`
	Children    []uiNode `xml:"node"`
}

func avdNodes() ([]uiNode, error) {
	if _, err := avdOutput("shell", "uiautomator", "dump", "/sdcard/astral-ui.xml"); err != nil {
		return nil, err
	}
	dump, err := avdOutput("shell", "cat", "/sdcard/astral-ui.xml")
	if err != nil {
		return nil, err
	}
	var root struct {
		Nodes []uiNode `xml:"node"`
	}
	if err := xml.Unmarshal([]byte(dump), &root); err != nil {
		return nil, err
	}
	var flat []uiNode
	var walk func(nodes []uiNode)
	walk = func(nodes []uiNode) {
		for _, node := range nodes {
			flat = append(flat, node)
			walk(node.Children)
		}
	}
	walk(root.Nodes)
	return flat, nil
}

func nodeCenter(node uiNode) (int, int, bool) {
	parts := regexp.MustCompile(`\d+`).FindAllString(node.Bounds, -1)
	if len(parts) != 4 {
		return 0, 0, false
	}
	values := make([]int, 4)
	for index, part := range parts {
		values[index], _ = strconv.Atoi(part)
	}
	return (values[0] + values[2]) / 2, (values[1] + values[3]) / 2, values[2] > values[0] && values[3] > values[1]
}

func avdFind(match func(uiNode) bool) (uiNode, bool, error) {
	nodes, err := avdNodes()
	if err != nil {
		return uiNode{}, false, err
	}
	for _, node := range nodes {
		if match(node) {
			return node, true, nil
		}
	}
	return uiNode{}, false, nil
}

func avdTapNode(node uiNode) error {
	x, y, ok := nodeCenter(node)
	if !ok {
		return fmt.Errorf("the view has no usable bounds: %s", node.Bounds)
	}
	if err := avdShell("shell", "input", "tap", strconv.Itoa(x), strconv.Itoa(y)); err != nil {
		return err
	}
	time.Sleep(2 * time.Second)
	return nil
}

func avdTap(target string) error {
	node, found, err := avdFind(func(node uiNode) bool {
		return node.Text == target || node.ResourceID == target || node.Description == target
	})
	if err != nil {
		return err
	}
	if !found {
		return fmt.Errorf("no view shows %q", target)
	}
	return avdTapNode(node)
}

func avdSettings(row string) error {
	if !strings.Contains(avdTopActivity(), discordActivity) {
		if err := avdLaunch(); err != nil {
			return err
		}
	}
	for attempt := 0; attempt < 4; attempt++ {
		node, found, err := avdFind(func(node uiNode) bool { return node.ResourceID == "settings-phone" || node.Text == "Account Settings" })
		if err != nil {
			return err
		}
		if found && node.Bounds != "" {
			break
		}
		profile, found, err := avdFind(func(node uiNode) bool { return node.Text == "Settings" && node.ResourceID == "" })
		if err != nil {
			return err
		}
		if found {
			if err := avdTapNode(profile); err != nil {
				return err
			}
			continue
		}
		you, found, err := avdFind(func(node uiNode) bool {
			return strings.HasPrefix(node.ResourceID, "you-bar") || strings.Contains(node.Description, "User Settings")
		})
		if err != nil {
			return err
		}
		if found {
			if err := avdTapNode(you); err != nil {
				return err
			}
			continue
		}
		avdShell("shell", "input", "keyevent", "KEYCODE_BACK")
		time.Sleep(2 * time.Second)
	}
	if row == "" {
		return nil
	}
	return avdTap("settings.row.astral." + row)
}

func avdLogs(args []string) error {
	flags := newFlags("avd logs")
	since := flags.Int("since", 60, "seconds of history")
	if err := flags.Parse(args); err != nil {
		return err
	}
	start := time.Now().Add(-time.Duration(*since) * time.Second).Format("01-02 15:04:05.000")
	listing, err := avdOutput("logcat", "-d", "-T", start, "-s", "astral:*")
	if err != nil {
		return err
	}
	fmt.Print(listing)
	return nil
}

func avdOpen(link string) error {
	if err := avdShell("shell", "am", "start", "-a", "android.intent.action.VIEW", "-d", link, discordPackage); err != nil {
		return err
	}
	time.Sleep(3 * time.Second)
	return nil
}
