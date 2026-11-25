package main

import (
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const replayUsage = "usage: astral replay <plugin folder>... <capture directory or .bin batches>"

func replayInputs(args []string) ([]string, []string, error) {
	var plugins, batches []string
	for _, arg := range args {
		info, err := os.Stat(arg)
		if err != nil {
			return nil, nil, err
		}
		switch {
		case info.IsDir() && fileExists(filepath.Join(arg, "manifest.toml")):
			plugins = append(plugins, arg)
		case info.IsDir():
			found, err := filepath.Glob(filepath.Join(arg, "*.bin"))
			if err != nil {
				return nil, nil, err
			}
			sort.Strings(found)
			batches = append(batches, found...)
		case strings.HasSuffix(arg, ".bin"):
			batches = append(batches, arg)
		}
	}
	if len(plugins) == 0 || len(batches) == 0 {
		return nil, nil, errors.New(replayUsage)
	}
	return plugins, batches, nil
}

func buildReplay(work string) (string, error) {
	lua := filepath.Join(work, "lua.o")
	if err := run(repoPath(), "cc", "-c", "-O1", "-DMAKE_LIB", luaPlatform(), "-w", repoPath("vendor", "lua", "onelua.c"), "-o", lua); err != nil {
		return "", err
	}
	objects := []string{lua}
	binary := filepath.Join(work, "replay")
	arguments := []string{"-std=c++20", "-O1", "-I" + repoPath("packages", "core", "include"), "-I" + repoPath("vendor", "lua"),
		"-I" + repoPath("packages", "core", "wire"), "-isystem", repoPath("vendor", "json", "single_include"),
		"-isystem", repoPath("vendor", "tomlplusplus", "include"), repoPath("packages", "core", "replay", "replay.cpp")}
	for _, source := range []string{"LuaSandbox", "LuaJSON", "LuaLocale", "PluginPackage"} {
		arguments = append(arguments, repoPath("packages", "core", source+".cpp"))
	}
	library, err := rustHostLibrary()
	if err != nil {
		return "", err
	}
	arguments = append(arguments, objects...)
	arguments = append(arguments, library, "-lm", "-o", binary)
	return binary, run(repoPath(), "clang++", arguments...)
}

func runReplay(args []string) error {
	plugins, batches, err := replayInputs(args)
	if err != nil {
		return err
	}
	work, err := os.MkdirTemp("", "astral-replay-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	binary, err := buildReplay(work)
	if err != nil {
		return err
	}
	arguments := append([]string{repoPath("packages", "core", "lua", "prelude.lua")}, plugins...)
	arguments = append(append(arguments, "--"), batches...)
	return run(repoPath(), binary, arguments...)
}
