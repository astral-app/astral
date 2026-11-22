package main

import (
	"encoding/base64"
	"errors"
	"flag"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

func runVphoneScreenshot(client vphoneClient, args []string) error {
	target := "vphone-screenshot.jpg"
	if len(args) > 0 {
		target = args[0]
	}
	var shot struct {
		Data string `json:"data"`
	}
	if err := client.call("screen.screenshot", map[string]any{"max_dimension": 1400, "quality": 0.8}, &shot); err != nil {
		return err
	}
	image, err := base64.StdEncoding.DecodeString(shot.Data)
	if err != nil {
		return err
	}
	if err := os.WriteFile(target, image, 0o644); err != nil {
		return err
	}
	fmt.Println(target)
	return nil
}

func wireDumpTool(destination string) (string, error) {
	tool := filepath.Join(destination, ".wire-dump")
	support := repoPath("packages", "ios", "tests", "support")
	return tool, run(repoPath(), "clang", "-O1", "-o", tool, filepath.Join(support, "wire-dump.c"), filepath.Join(support, "wire-parser.c"))
}

func pullCapturedBatches(client vphoneClient, wire string, destination string) ([]string, error) {
	entries, err := client.list(wire)
	if err != nil {
		return nil, err
	}
	var batches []string
	for _, entry := range entries {
		if !strings.HasSuffix(entry.Name, ".bin") {
			continue
		}
		local := filepath.Join(destination, entry.Name)
		if err := client.pull(path.Join(wire, entry.Name), local); err != nil {
			return nil, err
		}
		batches = append(batches, local)
	}
	sort.Strings(batches)
	return batches, nil
}

func restartDiscord(client vphoneClient) error {
	client.call("apps.terminate", map[string]any{"bundle_id": discordBundleID}, nil)
	return launchWhenStopped(client)
}

func runVphoneCapture(client vphoneClient, args []string) error {
	flags := flag.NewFlagSet("capture", flag.ContinueOnError)
	seconds := flags.Int("seconds", 30, "how long to record after Discord relaunches")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 1 {
		return errors.New("usage: astral vphone capture [--seconds 30] <output directory>")
	}
	destination := flags.Arg(0)
	if err := os.MkdirAll(destination, 0o755); err != nil {
		return err
	}
	data, err := discordDataPath(client)
	if err != nil {
		return err
	}
	state := path.Join(data, "Documents", "astral")
	marker := path.Join(state, "capture-wire")
	if _, err := vphoneRun("vphone", "touch "+shellQuote(marker), false); err != nil {
		return fmt.Errorf("turn on wire capture: %w", err)
	}
	defer func() {
		vphoneRun("vphone", "rm -f "+shellQuote(marker), false)
		restartDiscord(client)
	}()
	if err := restartDiscord(client); err != nil {
		return err
	}
	fmt.Printf("recording Phoenix's ViewWire batches for %ds; use Discord on the vphone now\n", *seconds)
	time.Sleep(time.Duration(*seconds) * time.Second)
	batches, err := pullCapturedBatches(client, path.Join(state, "wire"), destination)
	if err != nil {
		return err
	}
	if len(batches) == 0 {
		return errors.New("no batches were captured; is astral deployed on the vphone?")
	}
	tool, err := wireDumpTool(destination)
	if err != nil {
		return err
	}
	dump, err := output(repoPath(), tool, batches...)
	if err != nil {
		return err
	}
	jsonl := filepath.Join(destination, "wire.jsonl")
	if err := os.WriteFile(jsonl, []byte(dump), 0o644); err != nil {
		return err
	}
	fmt.Printf("%d batches in %s\n", len(batches), jsonl)
	return nil
}
