package main

import (
	"os"
	"path/filepath"
	"strings"
)

func rustCrate() string {
	return repoPath("packages", "core")
}

func rustHostLibrary() (string, error) {
	if err := run(rustCrate(), "cargo", "build", "--quiet"); err != nil {
		return "", err
	}
	return filepath.Join(rustCrate(), "target", "debug", "libastral_core.a"), nil
}

func rustHostReleaseLibrary() (string, error) {
	if err := run(rustCrate(), "cargo", "build", "--release", "--quiet"); err != nil {
		return "", err
	}
	return filepath.Join(rustCrate(), "target", "release", "libastral_core.a"), nil
}

func testRust(work string) error {
	return run(rustCrate(), "cargo", "test", "--quiet")
}

func rustTargetLibrary(target string) (string, error) {
	if err := run(rustCrate(), "cargo", "build", "--release", "--quiet", "--target", target); err != nil {
		return "", err
	}
	return filepath.Join(rustCrate(), "target", target, "release", "libastral_core.a"), nil
}

func rustObjects(archiver string, library string, directory string) ([]string, error) {
	extracted := filepath.Join(directory, "rust")
	if err := os.MkdirAll(extracted, 0o755); err != nil {
		return nil, err
	}
	if err := run(extracted, archiver, "x", library); err != nil {
		return nil, err
	}
	entries, err := os.ReadDir(extracted)
	if err != nil {
		return nil, err
	}
	var objects []string
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".o") {
			objects = append(objects, filepath.Join(extracted, entry.Name()))
		}
	}
	return objects, nil
}

func lintRust() error {
	if err := run(rustCrate(), "cargo", "fmt", "--check"); err != nil {
		return err
	}
	return run(rustCrate(), "cargo", "clippy", "--quiet", "--all-targets", "--", "-D", "warnings")
}

func formatRust() error {
	return run(rustCrate(), "cargo", "fmt")
}
