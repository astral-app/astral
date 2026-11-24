package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"

	"github.com/gorilla/websocket"
	"howett.net/plist"
)

const vphoneUsage = `usage: astral vphone <command>
  create [--ipa <IPA>] [--build 24B156]  build a VM with the adhoc preset, bootstrap, ElleKit and optionally Discord and astral
  snapshot <save|revert|list> [name]      stop, snapshot or revert, and boot again
  watch                            rebuild and deploy astral on every source change
  crashes [--process Discord]      the newest crash report, with astral frames symbolicated
  capture [--seconds 30] <dir>     record Phoenix's ViewWire batches while you use Discord, decoded to <dir>/wire.jsonl
  screenshot [file]                save the screen as a JPEG (vphone-screenshot.jpg by default)
  open <target>                    open a Discord link: user <id>, channel <guild> <channel> [message], dm [channel], settings [section] or a URL
  boot [--name vphone]              restart the VM with its guest API on, using a stored token
  status                           bootstrap state and the frontmost app
  call <method> [json params]      call any guest API method
  setup                            install ElleKit on a fresh bootstrap and check that injection works
  ssh [--root] <command>           run a command in the guest over SSH (mobile, or root with --root)
  session <save|restore>           back up or restore Discord's login (kept in ~/.config/astral/vphone-session)
  install <ipa>                    upload and install an IPA
  launch <bundle id>               open an app
  info <bundle id>                 an app's version, bundle and data paths
  push <local file> <guest path>   copy a file into the guest
  pull <guest path> <local file>   copy a file out of the guest
  discord --ipa <Discord.ipa>      install stock Discord, keeping the saved login
  deploy                           build astral, install it as an ElleKit tweak and relaunch Discord
The guest API listens on VPHONE_API (default 127.0.0.1:8765) with the token in VPHONE_API_TOKEN or ~/.config/astral/vphone-token.`

type vphoneClient struct {
	address string
	token   string
	http    *http.Client
}

type vphoneResponse struct {
	ID     int             `json:"id"`
	Type   string          `json:"type"`
	Result json.RawMessage `json:"result"`
	Error  *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
		Hint    string `json:"hint"`
	} `json:"error"`
}

func vphoneTokenPath() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "astral", "vphone-token")
}

func newVphoneClient() (vphoneClient, error) {
	address := os.Getenv("VPHONE_API")
	if address == "" {
		address = "127.0.0.1:8765"
	}
	token := os.Getenv("VPHONE_API_TOKEN")
	if token == "" {
		data, err := os.ReadFile(vphoneTokenPath())
		if err != nil {
			return vphoneClient{}, errors.New("no vphone API token; run astral vphone boot")
		}
		token = strings.TrimSpace(string(data))
	}
	return vphoneClient{address, token, &http.Client{Timeout: 30 * time.Minute}}, nil
}

func (v vphoneClient) call(method string, params map[string]any, result any) error {
	header := http.Header{"Authorization": {"Bearer " + v.token}}
	conn, _, err := websocket.DefaultDialer.Dial("ws://"+v.address+"/v1/events", header)
	if err != nil {
		return fmt.Errorf("vphone API at %s: %w", v.address, err)
	}
	defer conn.Close()
	if params == nil {
		params = map[string]any{}
	}
	if err := conn.WriteJSON(map[string]any{"type": "request", "id": 1, "method": method, "params": params}); err != nil {
		return err
	}
	deadline := time.Now().Add(30 * time.Minute)
	for {
		conn.SetReadDeadline(deadline)
		var response vphoneResponse
		if err := conn.ReadJSON(&response); err != nil {
			return err
		}
		if response.Type != "response" || response.ID != 1 {
			continue
		}
		if response.Error != nil {
			message := method + ": " + response.Error.Message
			if response.Error.Hint != "" {
				message += " (" + response.Error.Hint + ")"
			}
			return errors.New(message)
		}
		if result != nil && len(response.Result) > 0 {
			return json.Unmarshal(response.Result, result)
		}
		return nil
	}
}

func (v vphoneClient) contentURL(path string) string {
	return "http://" + v.address + "/v1/files/content?path=" + url.QueryEscape(path)
}

func (v vphoneClient) push(local string, remote string) error {
	file, err := os.Open(local)
	if err != nil {
		return err
	}
	defer file.Close()
	request, err := http.NewRequest(http.MethodPut, v.contentURL(remote), file)
	if err != nil {
		return err
	}
	request.Header.Set("Authorization", "Bearer "+v.token)
	response, err := v.http.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	body, _ := io.ReadAll(response.Body)
	if response.StatusCode != http.StatusOK {
		return fmt.Errorf("upload %s: %s", remote, strings.TrimSpace(string(body)))
	}
	return nil
}

func (v vphoneClient) pull(remote string, local string) error {
	request, err := http.NewRequest(http.MethodGet, v.contentURL(remote), nil)
	if err != nil {
		return err
	}
	request.Header.Set("Authorization", "Bearer "+v.token)
	response, err := v.http.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(response.Body)
		return fmt.Errorf("download %s: %s", remote, strings.TrimSpace(string(body)))
	}
	created, err := os.Create(local)
	if err != nil {
		return err
	}
	defer created.Close()
	_, err = io.Copy(created, response.Body)
	return err
}

func (v vphoneClient) install(ipa string) error {
	remote := "/var/root/Library/Caches/astral-install.ipa"
	if err := v.push(ipa, remote); err != nil {
		return err
	}
	defer v.call("files.remove", map[string]any{"path": remote}, nil)
	return v.call("apps.install", map[string]any{"path": remote}, nil)
}

func waitForFreePort(address string) {
	for attempt := 0; attempt < 60; attempt++ {
		listener, err := net.Listen("tcp", address)
		if err == nil {
			listener.Close()
			return
		}
		time.Sleep(500 * time.Millisecond)
	}
}

func bootVphone(name string) error {
	token := ""
	if data, err := os.ReadFile(vphoneTokenPath()); err == nil {
		token = strings.TrimSpace(string(data))
	}
	if token == "" {
		random, err := output(repoPath(), "openssl", "rand", "-hex", "16")
		if err != nil {
			return err
		}
		token = strings.TrimSpace(random)
		if err := os.MkdirAll(filepath.Dir(vphoneTokenPath()), 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(vphoneTokenPath(), []byte(token+"\n"), 0o600); err != nil {
			return err
		}
	}
	if running, _ := output(repoPath(), "pgrep", "-f", "emulator.*-avd"); strings.TrimSpace(running) != "" {
		return errors.New("the Android emulator is running; stop it first (the Mac cannot run both)")
	}
	exec.Command("vphone-cli", "vm", "stop", name).Run()
	waitForFreePort("127.0.0.1:8765")
	home, _ := os.UserHomeDir()
	config := filepath.Join(home, ".vphone", "machines", name, "config.plist")
	logPath := filepath.Join(os.TempDir(), "astral-vphone-"+name+".log")
	logFile, err := os.Create(logPath)
	if err != nil {
		return err
	}
	command := exec.Command("vphone-cli", "boot", "--config", config, "--api-listen", "127.0.0.1:8765")
	command.Dir = filepath.Dir(config)
	command.Env = append(os.Environ(), "VPHONE_API_TOKEN="+token)
	command.Stdout = logFile
	command.Stderr = logFile
	if err := command.Start(); err != nil {
		return err
	}
	client := vphoneClient{"127.0.0.1:8765", token, &http.Client{Timeout: time.Minute}}
	for attempt := 0; attempt < 90; attempt++ {
		var status map[string]any
		if client.call("bootstrap.status", nil, &status) == nil {
			fmt.Printf("%s is up; guest API on 127.0.0.1:8765, log in %s\n", name, logPath)
			return nil
		}
		time.Sleep(2 * time.Second)
	}
	return errors.New("the guest API did not come up; see " + logPath)
}

func vphoneReadyIPA(source string, work string) (string, error) {
	reader, err := zip.OpenReader(source)
	if err != nil {
		return "", err
	}
	defer reader.Close()
	target := filepath.Join(work, "vphone.ipa")
	created, err := os.Create(target)
	if err != nil {
		return "", err
	}
	writer := zip.NewWriter(created)
	for _, file := range reader.File {
		parts := strings.Split(file.Name, "/")
		if len(parts) >= 3 && strings.HasSuffix(parts[1], ".app") && (parts[2] == "PlugIns" || parts[2] == "Watch" || parts[2] == "Extensions") {
			continue
		}
		header := file.FileHeader
		destination, err := writer.CreateHeader(&header)
		if err != nil {
			return "", err
		}
		source, err := file.Open()
		if err != nil {
			return "", err
		}
		if len(parts) == 3 && strings.HasSuffix(parts[1], ".app") && parts[2] == "Info.plist" {
			data, err := io.ReadAll(source)
			source.Close()
			if err != nil {
				return "", err
			}
			var values map[string]any
			format, err := plist.Unmarshal(data, &values)
			if err != nil {
				return "", err
			}
			delete(values, "UISupportedDevices")
			encoded, err := plist.Marshal(values, format)
			if err != nil {
				return "", err
			}
			if _, err := io.Copy(destination, bytes.NewReader(encoded)); err != nil {
				return "", err
			}
			continue
		}
		_, copyErr := io.Copy(destination, source)
		source.Close()
		if copyErr != nil {
			return "", copyErr
		}
	}
	if err := writer.Close(); err != nil {
		return "", err
	}
	return target, created.Close()
}

func runVphone(args []string) error {
	if len(args) == 0 {
		return errors.New(vphoneUsage)
	}
	switch args[0] {
	case "create":
		return runVphoneCreate(args[1:])
	case "snapshot":
		return runVphoneSnapshot(args[1:])
	}
	if args[0] == "boot" {
		flags := newFlags("vphone boot")
		name := flags.String("name", "vphone", "VM name")
		if err := flags.Parse(args[1:]); err != nil {
			return err
		}
		return bootVphone(*name)
	}
	client, err := newVphoneClient()
	if err != nil {
		return err
	}
	switch args[0] {
	case "call":
		if len(args) < 2 || len(args) > 3 {
			return errors.New(vphoneUsage)
		}
		params := map[string]any{}
		if len(args) == 3 {
			if err := json.Unmarshal([]byte(args[2]), &params); err != nil {
				return fmt.Errorf("params must be a JSON object: %w", err)
			}
		}
		var result any
		if err := client.call(args[1], params, &result); err != nil {
			return err
		}
		encoded, _ := json.MarshalIndent(result, "", "  ")
		fmt.Println(string(encoded))
		return nil
	case "status":
		var status any
		if err := client.call("bootstrap.status", nil, &status); err != nil {
			return err
		}
		encoded, _ := json.MarshalIndent(status, "", "  ")
		fmt.Println(string(encoded))
		return nil
	case "install":
		if len(args) != 2 {
			return errors.New(vphoneUsage)
		}
		return client.install(args[1])
	case "info":
		if len(args) != 2 {
			return errors.New(vphoneUsage)
		}
		var info any
		if err := client.call("apps.info", map[string]any{"bundle_id": args[1]}, &info); err != nil {
			return err
		}
		encoded, _ := json.MarshalIndent(info, "", "  ")
		fmt.Println(string(encoded))
		return nil
	case "launch":
		if len(args) != 2 {
			return errors.New(vphoneUsage)
		}
		return client.call("apps.launch", map[string]any{"bundle_id": args[1]}, nil)
	case "push":
		if len(args) != 3 {
			return errors.New(vphoneUsage)
		}
		return client.push(args[1], args[2])
	case "pull":
		if len(args) != 3 {
			return errors.New(vphoneUsage)
		}
		return client.pull(args[1], args[2])
	case "deploy":
		return vphoneDeploy(client, args[1:])
	case "session":
		return runVphoneSession(client, args[1:])
	case "ssh":
		return runVphoneSSHCommand(args[1:])
	case "watch":
		return runVphoneWatch(client)
	case "crashes":
		return runVphoneCrashes(client, args[1:])
	case "capture":
		return runVphoneCapture(client, args[1:])
	case "screenshot":
		return runVphoneScreenshot(client, args[1:])
	case "open":
		link, err := discordLink(args[1:])
		if err != nil {
			return err
		}
		client.call("screen.unlock", map[string]any{"timeout_ms": 5000}, nil)
		return client.call("apps.launch", map[string]any{"bundle_id": discordBundleID, "url": link}, nil)
	case "setup":
		return runVphoneSetup(args[1:])
	case "discord":
		return vphoneInstallDiscord(client, args[1:])
	}
	return errors.New(vphoneUsage)
}

func vphoneUDID(name string) (string, error) {
	home, _ := os.UserHomeDir()
	data, err := os.ReadFile(filepath.Join(home, ".vphone", "machines", name, "udid-prediction.txt"))
	if err != nil {
		return "", err
	}
	for _, line := range strings.Split(string(data), "\n") {
		if value, found := strings.CutPrefix(strings.TrimSpace(line), "UDID="); found {
			return value, nil
		}
	}
	return "", errors.New("no UDID for " + name)
}

func installedTeam(client vphoneClient, bundleID string) string {
	var info struct {
		Entitlements map[string]any `json:"entitlements"`
	}
	if client.call("apps.info", map[string]any{"bundle_id": bundleID}, &info) != nil {
		return ""
	}
	team, _ := info.Entitlements["com.apple.developer.team-identifier"].(string)
	return team
}

func adhocSign(ipa string, team string, bundleID string, work string) (string, error) {
	extracted := filepath.Join(work, "adhoc")
	if err := unzip(ipa, extracted); err != nil {
		return "", err
	}
	apps, _ := filepath.Glob(filepath.Join(extracted, "Payload", "*.app"))
	if len(apps) != 1 {
		return "", errors.New("the IPA has no single Payload/*.app")
	}
	app := apps[0]
	entitlements := map[string]any{
		"application-identifier":                team + "." + bundleID,
		"com.apple.developer.team-identifier":   team,
		"get-task-allow":                        true,
		"keychain-access-groups":                []string{team + "." + bundleID},
		"com.apple.security.application-groups": []string{"group." + bundleID},
	}
	encoded, err := plist.MarshalIndent(entitlements, plist.XMLFormat, "\t")
	if err != nil {
		return "", err
	}
	entitlementsPath := filepath.Join(work, "adhoc.entitlements")
	if err := os.WriteFile(entitlementsPath, encoded, 0o644); err != nil {
		return "", err
	}
	if err := signBundle(app, "-", entitlementsPath); err != nil {
		return "", err
	}
	signed := filepath.Join(work, "adhoc.ipa")
	if err := run(extracted, "zip", "-qr", "-0", signed, "Payload"); err != nil {
		return "", err
	}
	return signed, nil
}

func vphoneDeploy(client vphoneClient, args []string) error {
	if len(args) > 0 {
		return errors.New("usage: astral vphone deploy (install Discord first with astral vphone discord --ipa <IPA>)")
	}
	return vphoneTweakDeploy(client)
}

func vphoneInstallDiscord(client vphoneClient, args []string) error {
	flags := newFlags("vphone discord")
	ipa := flags.String("ipa", "", "decrypted Discord IPA")
	name := flags.String("name", "vphone", "VM name")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *ipa == "" {
		return errors.New("usage: astral vphone discord --ipa <decrypted Discord.ipa> [--name vphone]")
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	ready, err := vphoneReadyIPA(*ipa, work)
	if err != nil {
		return err
	}
	team := installedTeam(client, discordBundleID)
	if team != "" {
		if saved, err := saveVphoneSession(client); err == nil {
			fmt.Printf("saved the login (%d files)\n", saved)
		}
		udid, err := vphoneUDID(*name)
		if err != nil {
			return err
		}
		signed, err := adhocSign(ready, team, discordBundleID, work)
		if err != nil {
			return err
		}
		if err := run(work, "ideviceinstaller", "-u", udid, "install", signed); err != nil {
			return err
		}
	} else if err := client.install(ready); err != nil {
		return err
	}
	if restored, err := restoreVphoneSession(client, *name); err == nil {
		fmt.Printf("restored the login (%d files)\n", restored)
	}
	fmt.Println("installed stock Discord on the vphone; run astral vphone deploy to add astral")
	return nil
}
