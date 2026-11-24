package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

func runTest(args []string) error {
	flags := newFlags("test")
	phoenix := flags.String("phoenix", os.Getenv("ASTRAL_PHOENIX_BINARY"), "PhoenixFatClient binary for the locator test")
	if err := flags.Parse(args); err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	steps := []func(string) error{testTools, testWire, testCore, testLua, testSignatures, testReadme}
	if *phoenix != "" {
		steps = append(steps, func(work string) error { return testLocator(work, *phoenix) })
	}
	for _, step := range steps {
		if err := step(work); err != nil {
			return err
		}
	}
	return nil
}

func testTools(work string) error {
	return run(repoPath("packages", "tools"), "go", "test", "./...")
}

func testWire(work string) error {
	ios := repoPath("packages", "ios")
	binary := filepath.Join(work, "phoenix-wire-test")
	arguments := []string{"-Wall", "-Wextra", "-Werror", "-g", "-fsanitize=address,undefined", "tests/phoenix-wire-test.c", "tests/support/wire-parser.c"}
	arguments = append(append(arguments, wireSources()...), "-lm", "-o", binary)
	if err := run(ios, "clang", arguments...); err != nil {
		return err
	}
	return run(ios, binary)
}

func wireSourceFiles() []string {
	paths, _ := filepath.Glob(repoPath("packages", "core", "wire", "*.c"))
	files := make([]string, len(paths))
	for index, path := range paths {
		files[index] = filepath.Base(path)
	}
	return files
}

func wireSources() []string {
	paths := []string{"-I" + repoPath("packages", "core", "wire")}
	for _, file := range wireSourceFiles() {
		paths = append(paths, repoPath("packages", "core", "wire", file))
	}
	return paths
}

func testCore(work string) error {
	object := filepath.Join(work, "lua-core.o")
	if err := run(repoPath(), "cc", "-c", "-O1", "-g", "-fsanitize=address,undefined", "-DMAKE_LIB", luaPlatform(), "-w",
		repoPath("vendor", "lua", "onelua.c"), "-o", object); err != nil {
		return err
	}
	if err := runEmbedPrelude([]string{repoPath("packages", "core", "lua", "prelude.lua"), filepath.Join(work, "AstralPrelude.h")}); err != nil {
		return err
	}
	var wireObjects []string
	monocypher := repoPath("vendor", "monocypher", "src")
	var nativeSources []string
	for _, name := range wireSourceFiles() {
		nativeSources = append(nativeSources, repoPath("packages", "core", "wire", name))
	}
	nativeSources = append(nativeSources, filepath.Join(monocypher, "monocypher.c"), filepath.Join(monocypher, "optional", "monocypher-ed25519.c"))
	for _, source := range nativeSources {
		target := filepath.Join(work, filepath.Base(source)+".o")
		if err := run(repoPath(), "clang", "-std=c11", "-g", "-fsanitize=address,undefined", "-I"+monocypher, "-I"+filepath.Join(monocypher, "optional"), "-I"+repoPath("packages", "core", "wire"), "-c", source, "-o", target); err != nil {
			return err
		}
		wireObjects = append(wireObjects, target)
	}
	binary := filepath.Join(work, "lua-core-test")
	arguments := []string{"-std=c++20", "-Wall", "-Wextra", "-Werror", "-g", "-fsanitize=address,undefined",
		"-I" + repoPath("packages", "core", "include"), "-I" + repoPath("vendor", "lua"), "-I" + work, "-I" + repoPath("packages", "core", "wire"),
		"-isystem", repoPath("vendor", "json", "single_include"), "-isystem", repoPath("vendor", "tomlplusplus", "include"),
		repoPath("packages", "core", "tests", "lua-core-test.cpp")}
	sources, err := portableCoreSources()
	if err != nil {
		return err
	}
	arguments = append(arguments, sources...)
	arguments = append(arguments, wireObjects...)
	arguments = append(arguments, "-I"+monocypher, "-I"+filepath.Join(monocypher, "optional"))
	if runtime.GOOS != "darwin" {
		arguments = append(arguments, "-lcrypto")
	}
	arguments = append(arguments, object, "-lsqlite3", "-o", binary)
	if err := run(repoPath(), "clang++", arguments...); err != nil {
		return err
	}
	return run(repoPath(), binary, work, repoPath())
}

func luaPlatform() string {
	if runtime.GOOS == "darwin" {
		return "-DLUA_USE_MACOSX"
	}
	return "-DLUA_USE_LINUX"
}

func buildLua(work string) (string, error) {
	binary := filepath.Join(work, "lua")
	return binary, run(repoPath(), "cc", "-O1", luaPlatform(), "-w", "-o", binary, repoPath("vendor", "lua", "onelua.c"), "-lm")
}

func testLua(work string) error {
	lua, err := buildLua(work)
	if err != nil {
		return err
	}
	prelude := repoPath("packages", "core", "lua", "prelude.lua")
	if err := run(repoPath(), lua, repoPath("packages", "core", "tests", "prelude-test.lua"), prelude); err != nil {
		return err
	}
	folders, err := pluginFolders()
	if err != nil {
		return err
	}
	for _, folder := range folders {
		manifest, err := readManifest(folder)
		if err != nil {
			return err
		}
		tests, err := filepath.Glob(filepath.Join(folder, "tests", "*.lua"))
		if err != nil {
			return err
		}
		for _, test := range tests {
			if err := run(repoPath(), lua, test, filepath.Join(folder, manifest.Main), prelude); err != nil {
				return err
			}
		}
	}
	return nil
}

func testSignatures(work string) error {
	key, err := publicKey()
	if err != nil {
		return err
	}
	folders, err := pluginFolders()
	if err != nil {
		return err
	}
	for _, folder := range folders {
		if err := verifyPlugin(folder, key); err != nil {
			return err
		}
	}
	if err := verifyCatalog(key); err != nil {
		return err
	}
	return verifyWithMonocypher(work, folders)
}

func verifyWithMonocypher(work string, folders []string) error {
	monocypher := repoPath("vendor", "monocypher", "src")
	verifier := filepath.Join(work, "verify-signature")
	if err := run(repoPath(), "cc", "-O1", "-w", "-I"+monocypher, "-I"+filepath.Join(monocypher, "optional"), "-o", verifier,
		repoPath("packages", "ios", "tests", "support", "verify-signature.c"), filepath.Join(monocypher, "monocypher.c"),
		filepath.Join(monocypher, "optional", "monocypher-ed25519.c")); err != nil {
		return err
	}
	keyText, err := os.ReadFile(repoPath("keys", "plugin-signing.pub"))
	if err != nil {
		return err
	}
	key := strings.TrimSpace(string(keyText))
	for _, folder := range folders {
		signature, err := readPluginSignature(folder)
		if err != nil {
			return err
		}
		message := filepath.Join(work, "message")
		sig := filepath.Join(work, "signature")
		if err := os.WriteFile(message, signatureMessage(signature.Files), 0o644); err != nil {
			return err
		}
		if err := os.WriteFile(sig, []byte(signature.Signature), 0o644); err != nil {
			return err
		}
		if err := run(work, verifier, message, sig, key); err != nil {
			return fmt.Errorf("%s: Monocypher rejects the signature", filepath.Base(folder))
		}
	}
	if err := run(work, verifier, repoPath("packages", "catalog", "index.toml"), repoPath("packages", "catalog", "index.toml.sig"), key); err != nil {
		return fmt.Errorf("Monocypher rejects the catalog signature")
	}
	fmt.Println("signatures ok")
	return nil
}

func testLocator(work string, phoenix string) error {
	binary := filepath.Join(work, "locate-test")
	sources := repoPath("packages", "ios")
	if err := run(sources, "clang", "-Wall", "-Wextra", "-Werror", "-O1", "tests/locate-test.c", "sources/PhoenixLocate.c", "-I"+repoPath("packages", "core", "wire"), repoPath("packages", "core", "wire", "PhoenixProperties.c"), "-o", binary); err != nil {
		return err
	}
	return run(sources, binary, phoenix)
}
