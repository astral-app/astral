package main

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
)

func sdkDirectory() (string, error) {
	if data := os.Getenv("XDG_DATA_HOME"); data != "" {
		return filepath.Join(data, "astral", "sdk"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", "astral", "sdk"), nil
}

func installSDK() (string, error) {
	target, err := sdkDirectory()
	if err != nil {
		return "", err
	}
	source := repoPath("packages", "sdk")
	if err := os.RemoveAll(target); err != nil {
		return "", err
	}
	err = filepath.WalkDir(source, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || entry.IsDir() {
			return err
		}
		relative, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		return copyFile(path, filepath.Join(target, relative))
	})
	return target, err
}

func runSDK(args []string) error {
	if len(args) != 1 || args[0] != "install" {
		return errors.New("usage: astral sdk install")
	}
	target, err := installSDK()
	if err != nil {
		return err
	}
	fmt.Printf("installed the astral SDK in %s\nadd %q to workspace.library in your .luarc.json, or point Lua.workspace.userThirdParty at %q\n", target, filepath.Join(target, "library"), filepath.Dir(target))
	return nil
}
