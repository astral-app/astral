package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

var luaCompilerSources = []string{"lapi", "lcode", "lctype", "ldebug", "ldo", "ldump", "lfunc", "lgc", "llex", "lmem", "lobject",
	"lopcodes", "lparser", "lstate", "lstring", "ltable", "ltm", "lundump", "lvm", "lzio", "lauxlib"}

func luaCompiler() (string, error) {
	inputs := []string{repoPath("packages", "tools", "support", "compile-lua.c")}
	headers, err := filepath.Glob(repoPath("vendor", "lua", "*.h"))
	if err != nil {
		return "", err
	}
	for _, name := range luaCompilerSources {
		inputs = append(inputs, repoPath("vendor", "lua", name+".c"))
	}
	hash := sha256.New()
	for _, path := range append(append([]string{}, inputs...), headers...) {
		data, err := os.ReadFile(path)
		if err != nil {
			return "", err
		}
		hash.Write(data)
	}
	cache, err := os.UserCacheDir()
	if err != nil {
		return "", err
	}
	binary := filepath.Join(cache, "astral", "compile-lua-"+hex.EncodeToString(hash.Sum(nil))[:16])
	if fileExists(binary) {
		return binary, nil
	}
	if err := os.MkdirAll(filepath.Dir(binary), 0o755); err != nil {
		return "", err
	}
	arguments := append([]string{"-O2", "-I" + repoPath("vendor", "lua"), "-o", binary}, inputs...)
	if err := run(repoPath(), "clang", append(arguments, "-lm")...); err != nil {
		return "", fmt.Errorf("building the Lua compiler: %w", err)
	}
	return binary, nil
}

func compileLua(chunk, source, output string) error {
	compiler, err := luaCompiler()
	if err != nil {
		return err
	}
	return run(repoPath(), compiler, chunk, source, output)
}

func bytecodePath(source string) string {
	return strings.TrimSuffix(source, ".lua") + ".luac"
}

func moduleChunk(identifier, main, module string) string {
	if module == main {
		return "=" + identifier
	}
	return "=" + identifier + "/" + strings.ReplaceAll(strings.TrimSuffix(module, ".lua"), "/", ".")
}

func compilePlugin(folder string) error {
	manifest, err := readManifest(folder)
	if err != nil {
		return err
	}
	stale, err := filepath.Glob(filepath.Join(folder, "*.luac"))
	if err != nil {
		return err
	}
	nested, err := filepath.Glob(filepath.Join(folder, "*", "*.luac"))
	if err != nil {
		return err
	}
	for _, path := range append(stale, nested...) {
		if err := os.Remove(path); err != nil {
			return err
		}
	}
	modules, err := pluginModules(folder)
	if err != nil {
		return err
	}
	sort.Strings(modules)
	for _, module := range modules {
		source := filepath.Join(folder, filepath.FromSlash(module))
		if err := compileLua(moduleChunk(manifest.ID, manifest.Main, module), source, bytecodePath(source)); err != nil {
			return fmt.Errorf("%s: %w", module, err)
		}
	}
	return nil
}

func runEmbedPrelude(args []string) error {
	if len(args) != 2 {
		return fmt.Errorf("usage: astral embed-prelude <prelude.lua> <header>")
	}
	work, err := os.MkdirTemp("", "astral-prelude")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	compiled := filepath.Join(work, "prelude.luac")
	if err := compileLua("=astral", args[0], compiled); err != nil {
		return err
	}
	bytecode, err := os.ReadFile(compiled)
	if err != nil {
		return err
	}
	var header strings.Builder
	header.WriteString("static const unsigned char kAstralPrelude[] = {")
	for index, value := range bytecode {
		if index%24 == 0 {
			header.WriteString("\n    ")
		}
		fmt.Fprintf(&header, "%d,", value)
	}
	header.WriteString("\n};\n")
	if err := os.MkdirAll(filepath.Dir(args[1]), 0o755); err != nil {
		return err
	}
	if existing, err := os.ReadFile(args[1]); err == nil && string(existing) == header.String() {
		return nil
	}
	return os.WriteFile(args[1], []byte(header.String()), 0o644)
}
