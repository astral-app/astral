package main

import (
	"bytes"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func repoRoot() (string, error) {
	directory, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		if _, err := os.Stat(filepath.Join(directory, "go.work")); err == nil {
			if _, err := os.Stat(filepath.Join(directory, "packages", "loader-ios")); err == nil {
				return directory, nil
			}
		}
		parent := filepath.Dir(directory)
		if parent == directory {
			return "", errors.New("not inside the astral repository")
		}
		directory = parent
	}
}

func repoPath(parts ...string) string {
	root, err := repoRoot()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	return filepath.Join(append([]string{root}, parts...)...)
}

func run(directory string, name string, args ...string) error {
	command := exec.Command(name, args...)
	command.Dir = directory
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	if err := command.Run(); err != nil {
		return fmt.Errorf("%s %s: %w", name, strings.Join(args, " "), err)
	}
	return nil
}

func runWithEnv(directory string, env []string, name string, args ...string) error {
	command := exec.Command(name, args...)
	command.Dir = directory
	command.Env = append(os.Environ(), env...)
	command.Stdout = os.Stdout
	command.Stderr = os.Stderr
	if err := command.Run(); err != nil {
		return fmt.Errorf("%s %s: %w", name, strings.Join(args, " "), err)
	}
	return nil
}

func output(directory string, name string, args ...string) (string, error) {
	command := exec.Command(name, args...)
	command.Dir = directory
	var stderr bytes.Buffer
	command.Stderr = &stderr
	result, err := command.Output()
	if err != nil {
		return string(result), fmt.Errorf("%s %s: %w: %s", name, strings.Join(args, " "), err, strings.TrimSpace(stderr.String()))
	}
	return string(result), nil
}

func trackedFiles(patterns ...string) ([]string, error) {
	listing, err := output(repoPath(), "git", append([]string{"ls-files", "--"}, patterns...)...)
	if err != nil {
		return nil, err
	}
	return strings.Fields(listing), nil
}

func pluginFolders() ([]string, error) {
	manifests, err := filepath.Glob(repoPath("packages", "plugins", "*", "manifest.toml"))
	if err != nil {
		return nil, err
	}
	folders := make([]string, len(manifests))
	for index, manifest := range manifests {
		folders[index] = filepath.Dir(manifest)
	}
	return folders, nil
}

func newFlags(name string) *flag.FlagSet {
	return flag.NewFlagSet("astral "+name, flag.ContinueOnError)
}

func deviceFlag(flags *flag.FlagSet) *string {
	return flags.String("device", os.Getenv("DEVICE"), "test device UDID")
}

func requireDevice(device string) error {
	if device == "" {
		return errors.New("set --device or DEVICE to the test device UDID")
	}
	return nil
}

func temporaryDirectory() (string, func(), error) {
	directory, err := os.MkdirTemp("", "astral-")
	if err != nil {
		return "", nil, err
	}
	return directory, func() { os.RemoveAll(directory) }, nil
}
