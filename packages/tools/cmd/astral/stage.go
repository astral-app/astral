package main

import (
	"bytes"
	"errors"
	"io"
	"os"
	"path/filepath"
)

func copyFile(source string, destination string) error {
	if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
		return err
	}
	input, err := os.Open(source)
	if err != nil {
		return err
	}
	defer input.Close()
	created, err := os.Create(destination)
	if err != nil {
		return err
	}
	if _, err := io.Copy(created, input); err != nil {
		created.Close()
		return err
	}
	return created.Close()
}

func copyGlob(pattern string, destination string) error {
	matches, err := filepath.Glob(pattern)
	if err != nil {
		return err
	}
	for _, match := range matches {
		if err := copyFile(match, filepath.Join(destination, filepath.Base(match))); err != nil {
			return err
		}
	}
	return nil
}

func stagePlugin(folder string, destination string) error {
	paths, err := signedPaths(folder)
	if err != nil {
		return err
	}
	for _, path := range append(paths, "signature") {
		if err := copyFile(filepath.Join(folder, path), filepath.Join(destination, path)); err != nil {
			return err
		}
	}
	return nil
}

func stageResources(target string) error {
	if err := os.RemoveAll(target); err != nil {
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
		for _, parent := range []string{"plugins", filepath.Join("catalog", "plugins")} {
			if err := stagePlugin(folder, filepath.Join(target, parent, manifest.ID)); err != nil {
				return err
			}
		}
	}
	copies := [][2]string{
		{repoPath("packages", "addons", "themes", "*.toml"), filepath.Join(target, "themes")},
		{repoPath("packages", "addons", "themes", "palettes", "*.toml"), filepath.Join(target, "palettes")},
		{repoPath("packages", "catalog", "index.toml*"), filepath.Join(target, "catalog")},
		{repoPath("packages", "catalog", "themes", "*.toml"), filepath.Join(target, "catalog", "themes")},
		{repoPath("packages", "assets", "*.svg"), filepath.Join(target, "assets")},
	}
	emojiPacks, err := filepath.Glob(repoPath("packages", "addons", "emoji-packs", "*"))
	if err != nil {
		return err
	}
	for _, pack := range emojiPacks {
		copies = append(copies, [2]string{filepath.Join(pack, "*"), filepath.Join(target, "emoji-packs", filepath.Base(pack))})
	}
	packs, err := filepath.Glob(repoPath("packages", "addons", "icon-packs", "*"))
	if err != nil {
		return err
	}
	for _, pack := range packs {
		name := filepath.Base(pack)
		copies = append(copies,
			[2]string{filepath.Join(pack, "manifest.toml"), filepath.Join(target, "icon-packs", name)},
			[2]string{filepath.Join(pack, "icons", "*.png"), filepath.Join(target, "icon-packs", name, "icons")})
	}
	for _, entry := range copies {
		if err := copyGlob(entry[0], entry[1]); err != nil {
			return err
		}
	}
	return nil
}

func runStage(args []string) error {
	if len(args) != 1 {
		return errors.New("usage: astral stage <directory>")
	}
	return stageResources(args[0])
}

func runEmbedPrelude(args []string) error {
	if len(args) != 2 {
		return errors.New("usage: astral embed-prelude <prelude.lua> <header>")
	}
	source, err := os.ReadFile(args[0])
	if err != nil {
		return err
	}
	const delimiter = "astral_prelude"
	if bytes.Contains(source, []byte(")"+delimiter+"\"")) {
		return errors.New(args[0] + " contains the raw string delimiter")
	}
	header := []byte("static const char kAstralPrelude[] = R\"" + delimiter + "(" + string(source) + ")" + delimiter + "\";\n")
	if existing, err := os.ReadFile(args[1]); err == nil && bytes.Equal(existing, header) {
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(args[1]), 0o755); err != nil {
		return err
	}
	return os.WriteFile(args[1], header, 0o644)
}
