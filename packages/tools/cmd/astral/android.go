package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
)

const androidAPILevel = "26"

var androidABIs = map[string]string{"arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android"}

var appleOnlyCoreSources = map[string]bool{"AstralMetricKitCrash": true}

var luaLibrarySources = []string{"lapi", "lcode", "lctype", "ldebug", "ldo", "ldump", "lfunc", "lgc", "llex", "lmem", "lobject", "lopcodes", "lparser", "lstate", "lstring", "ltable", "ltm", "lundump", "lvm", "lzio", "lauxlib", "lbaselib", "lstrlib", "ltablib", "lmathlib", "lutf8lib", "lcorolib"}

func runAndroidCore(args []string) error {
	flags := newFlags("android-core")
	ndk := flags.String("ndk", "", "Android NDK directory")
	destination := flags.String("output", repoPath("packages", "core", "build", "android"), "directory for the static libraries")
	if err := flags.Parse(args); err != nil {
		return err
	}
	toolchain, err := ndkToolchain(*ndk)
	if err != nil {
		return err
	}
	sources, err := portableCoreSources()
	if err != nil {
		return err
	}
	abis := make([]string, 0, len(androidABIs))
	for abi := range androidABIs {
		abis = append(abis, abi)
	}
	sort.Strings(abis)
	for _, abi := range abis {
		library, err := buildAndroidCore(toolchain, androidABIs[abi], sources, filepath.Join(*destination, abi))
		if err != nil {
			return fmt.Errorf("%s: %w", abi, err)
		}
		fmt.Println(library)
	}
	return nil
}

func ndkToolchain(explicit string) (string, error) {
	host := "linux-x86_64"
	if runtime.GOOS == "darwin" {
		host = "darwin-x86_64"
	}
	for _, root := range ndkCandidates(explicit) {
		toolchain := filepath.Join(root, "toolchains", "llvm", "prebuilt", host, "bin")
		if _, err := os.Stat(filepath.Join(toolchain, "llvm-ar")); err == nil {
			return toolchain, nil
		}
	}
	return "", errors.New("no Android NDK; pass --ndk or set ANDROID_NDK_HOME")
}

func ndkCandidates(explicit string) []string {
	candidates := []string{explicit, os.Getenv("ANDROID_NDK_HOME"), os.Getenv("ANDROID_NDK_LATEST_HOME")}
	home, _ := os.UserHomeDir()
	installed, _ := filepath.Glob(filepath.Join(home, "Library", "Android", "sdk", "ndk", "*"))
	sort.Sort(sort.Reverse(sort.StringSlice(installed)))
	var present []string
	for _, candidate := range append(candidates, installed...) {
		if candidate != "" {
			present = append(present, candidate)
		}
	}
	return present
}

func portableCoreSources() ([]string, error) {
	files, err := filepath.Glob(repoPath("packages", "core", "src", "*.cpp"))
	if err != nil {
		return nil, err
	}
	var sources []string
	for _, file := range files {
		if !appleOnlyCoreSources[strings.TrimSuffix(filepath.Base(file), ".cpp")] {
			sources = append(sources, file)
		}
	}
	return sources, nil
}

func buildAndroidCore(toolchain string, triple string, sources []string, directory string) (string, error) {
	objects := filepath.Join(directory, "objects")
	if err := os.MkdirAll(objects, 0o755); err != nil {
		return "", err
	}
	compiler := filepath.Join(toolchain, triple+androidAPILevel+"-clang")
	generated := filepath.Join(directory, "generated")
	if err := os.MkdirAll(generated, 0o755); err != nil {
		return "", err
	}
	if err := runEmbedPrelude([]string{repoPath("packages", "core", "lua", "prelude.lua"), filepath.Join(generated, "AstralPrelude.h")}); err != nil {
		return "", err
	}
	includes := []string{"-I" + repoPath("packages", "core", "include"), "-I" + repoPath("packages", "core", "wire"), "-I" + generated,
		"-I" + repoPath("vendor", "lua"), "-I" + repoPath("vendor", "sqlite"), "-I" + repoPath("vendor", "monocypher", "src"), "-I" + repoPath("vendor", "monocypher", "src", "optional"),
		"-isystem", repoPath("vendor", "json", "single_include"), "-isystem", repoPath("vendor", "tomlplusplus", "include")}
	var built []string
	cSources, err := filepath.Glob(repoPath("packages", "core", "wire", "*.c"))
	if err != nil {
		return "", err
	}
	for _, source := range append(cSources, repoPath("vendor", "sqlite", "sqlite3.c"), repoPath("vendor", "monocypher", "src", "monocypher.c"), repoPath("vendor", "monocypher", "src", "optional", "monocypher-ed25519.c")) {
		object := filepath.Join(objects, "c-"+strings.TrimSuffix(filepath.Base(source), ".c")+".o")
		if err := run(repoPath(), compiler, append([]string{"-O2", "-fPIC", "-w", "-c", source, "-o", object}, includes...)...); err != nil {
			return "", err
		}
		built = append(built, object)
	}
	for _, source := range sources {
		object := filepath.Join(objects, strings.TrimSuffix(filepath.Base(source), ".cpp")+".o")
		arguments := append([]string{"-std=c++20", "-O2", "-fPIC", "-Wall", "-Werror", "-c", source, "-o", object}, includes...)
		if err := run(repoPath(), compiler+"++", arguments...); err != nil {
			return "", err
		}
		built = append(built, object)
	}
	for _, name := range luaLibrarySources {
		object := filepath.Join(objects, "lua-"+name+".o")
		if err := run(repoPath(), compiler, "-O2", "-fPIC", "-w", "-DLUA_USE_POSIX", "-c", repoPath("vendor", "lua", name+".c"), "-o", object); err != nil {
			return "", err
		}
		built = append(built, object)
	}
	library := filepath.Join(directory, "libastral-core.a")
	os.Remove(library)
	if err := run(repoPath(), filepath.Join(toolchain, "llvm-ar"), append([]string{"rcs", library}, built...)...); err != nil {
		return "", err
	}
	if err := checkAndroidLinks(compiler, library, directory); err != nil {
		return "", err
	}
	os.RemoveAll(generated)
	return library, os.RemoveAll(objects)
}

func checkAndroidLinks(compiler string, library string, directory string) error {
	shared := filepath.Join(directory, "libastral-core.so")
	defer os.Remove(shared)
	return run(repoPath(), compiler+"++", "-shared", "-static-libstdc++", "-Wl,--no-undefined", "-Wl,--whole-archive", library,
		"-Wl,--no-whole-archive", "-llog", "-lm", "-o", shared)
}
