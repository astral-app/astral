package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"time"
)

const devUsage = `usage: astral dev <command>
  status                 the device's astral version and plugins
  push <plugin folder>   install a plugin and hot reload it
  watch <plugin folder>  push the plugin every time a file in it changes
  eval <plugin id> <lua> run Lua inside a running plugin and print the result
  logs [-f]              print astral's log, following it with -f
  theme import <file>    convert an Unbound theme (JSON) into an astral theme and select it
  backup export [file]   write the device's settings backup (TOML) to a file or stdout
  backup import <file>   restore a settings backup on the device
Needs Developer mode on. With DEVICE set the port is forwarded over USB; without it, over SSH to the vphone.`

type devClient struct {
	base   string
	port   string
	client *http.Client
}

func newDevClient() devClient {
	port := os.Getenv("ASTRAL_DEV_PORT")
	if port == "" {
		port = "9324"
	}
	return devClient{"http://127.0.0.1:" + port, port, &http.Client{Timeout: 20 * time.Second}}
}

func (d devClient) reachable() bool {
	quick := &http.Client{Timeout: time.Second}
	response, err := quick.Get(d.base + "/status")
	if err != nil {
		return false
	}
	response.Body.Close()
	return response.StatusCode < 500
}

func (d devClient) forward() error {
	if d.reachable() {
		return nil
	}
	device := os.Getenv("DEVICE")
	if device == "" {
		return d.forwardOverVphoneSSH()
	}
	proxy := exec.Command("iproxy", d.port, d.port, "-u", device)
	if err := proxy.Start(); err != nil {
		return err
	}
	go proxy.Wait()
	for attempt := 0; attempt < 10; attempt++ {
		if d.reachable() {
			return nil
		}
		time.Sleep(500 * time.Millisecond)
	}
	return errors.New("astral's development server is not reachable; turn on Developer mode")
}

func (d devClient) forwardOverVphoneSSH() error {
	client, err := vphoneSSH("vphone")
	if err != nil {
		return fmt.Errorf("set DEVICE for a USB device, or boot the vphone: %w", err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:"+d.port)
	if err != nil {
		client.Close()
		return err
	}
	go func() {
		for {
			local, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				defer local.Close()
				remote, err := client.Dial("tcp", "127.0.0.1:"+d.port)
				if err != nil {
					return
				}
				defer remote.Close()
				go io.Copy(remote, local)
				io.Copy(local, remote)
			}()
		}
	}()
	if d.reachable() {
		return nil
	}
	return errors.New("astral's development server is not reachable on the vphone; turn on Developer mode and relaunch Discord")
}

func (d devClient) request(method string, path string, body any) ([]byte, error) {
	var reader io.Reader
	if body != nil {
		switch value := body.(type) {
		case []byte:
			reader = bytes.NewReader(value)
		default:
			encoded, err := json.Marshal(value)
			if err != nil {
				return nil, err
			}
			reader = bytes.NewReader(encoded)
		}
	}
	request, err := http.NewRequest(method, d.base+path, reader)
	if err != nil {
		return nil, err
	}
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := d.client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	return io.ReadAll(response.Body)
}

func (d devClient) push(folder string) error {
	manifest, err := readManifest(folder)
	if err != nil {
		return err
	}
	manifestText, err := os.ReadFile(filepath.Join(folder, "manifest.toml"))
	if err != nil {
		return err
	}
	main, err := os.ReadFile(filepath.Join(folder, manifest.Main))
	if err != nil {
		return err
	}
	modules := map[string]string{}
	paths, err := pluginModules(folder)
	if err != nil {
		return err
	}
	for _, path := range paths {
		if path == manifest.Main {
			continue
		}
		text, err := os.ReadFile(filepath.Join(folder, path))
		if err != nil {
			return err
		}
		modules[path] = string(text)
	}
	reply, err := d.request(http.MethodPut, "/plugin", map[string]any{"manifest": string(manifestText), "main": string(main), "modules": modules})
	if err != nil {
		return err
	}
	fmt.Println(string(reply))
	return nil
}

func latestChange(folder string) (time.Time, error) {
	var latest time.Time
	err := filepath.WalkDir(folder, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if info.ModTime().After(latest) {
			latest = info.ModTime()
		}
		return nil
	})
	return latest, err
}

func (d devClient) watch(folder string) error {
	if err := d.push(folder); err != nil {
		return err
	}
	seen, err := latestChange(folder)
	if err != nil {
		return err
	}
	for {
		time.Sleep(500 * time.Millisecond)
		changed, err := latestChange(folder)
		if err != nil {
			return err
		}
		if changed.After(seen) {
			seen = changed
			if err := d.push(folder); err != nil {
				fmt.Fprintln(os.Stderr, err)
			}
		}
	}
}

func (d devClient) logs(follow bool) error {
	since := 0
	for {
		reply, err := d.request(http.MethodGet, fmt.Sprintf("/logs?since=%d", since), nil)
		if err != nil {
			return err
		}
		var lines []struct {
			ID   int    `json:"id"`
			Line string `json:"line"`
		}
		if err := json.Unmarshal(reply, &lines); err != nil {
			return err
		}
		for _, entry := range lines {
			fmt.Fprintln(os.Stderr, entry.Line)
			since = entry.ID
		}
		if !follow {
			return nil
		}
		time.Sleep(time.Second)
	}
}

func printIndented(reply []byte) {
	var formatted bytes.Buffer
	if json.Indent(&formatted, reply, "", "  ") == nil {
		fmt.Println(formatted.String())
		return
	}
	fmt.Println(string(reply))
}

func (d devClient) backup(args []string) error {
	if len(args) == 0 {
		return errors.New(devUsage)
	}
	switch args[0] {
	case "export":
		reply, err := d.request(http.MethodGet, "/backup", nil)
		if err != nil {
			return err
		}
		var result struct {
			Backup string `json:"backup"`
		}
		if err := json.Unmarshal(reply, &result); err != nil {
			return err
		}
		if len(args) > 1 {
			return os.WriteFile(args[1], []byte(result.Backup), 0o644)
		}
		fmt.Print(result.Backup)
		return nil
	case "import":
		if len(args) < 2 {
			return errors.New(devUsage)
		}
		text, err := os.ReadFile(args[1])
		if err != nil {
			return err
		}
		reply, err := d.request(http.MethodPost, "/backup", map[string]string{"backup": string(text)})
		if err != nil {
			return err
		}
		fmt.Println(string(reply))
		return nil
	}
	return errors.New(devUsage)
}

func runDev(args []string) error {
	if len(args) == 0 {
		return errors.New(devUsage)
	}
	d := newDevClient()
	if err := d.forward(); err != nil {
		return err
	}
	switch args[0] {
	case "status":
		reply, err := d.request(http.MethodGet, "/status", nil)
		if err != nil {
			return err
		}
		printIndented(reply)
		return nil
	case "push":
		if len(args) < 2 {
			return errors.New(devUsage)
		}
		return d.push(args[1])
	case "watch":
		if len(args) < 2 {
			return errors.New(devUsage)
		}
		return d.watch(args[1])
	case "eval":
		if len(args) < 3 {
			return errors.New(devUsage)
		}
		reply, err := d.request(http.MethodPost, "/eval", map[string]string{"plugin": args[1], "code": args[2]})
		if err != nil {
			return err
		}
		var result struct {
			Result string `json:"result"`
		}
		if err := json.Unmarshal(reply, &result); err != nil {
			return err
		}
		fmt.Println(result.Result)
		return nil
	case "logs":
		return d.logs(len(args) > 1 && args[1] == "-f")
	case "theme":
		if len(args) < 3 || args[1] != "import" {
			return errors.New(devUsage)
		}
		theme, err := os.ReadFile(args[2])
		if err != nil {
			return err
		}
		reply, err := d.request(http.MethodPost, "/unbound-theme", theme)
		if err != nil {
			return err
		}
		fmt.Println(string(reply))
		return nil
	case "backup":
		return d.backup(args[1:])
	}
	return errors.New(devUsage)
}
