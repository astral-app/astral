package main

import (
	"bytes"
	"encoding/base64"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

const (
	readmeIconSize = 20
	readmeGrey     = "#8C8C94"
)

type readmePlugin struct {
	folder      string
	ID          string `toml:"id"`
	Name        string `toml:"name"`
	Description string `toml:"description"`
	Icon        string `toml:"icon"`
	IconSVG     string `toml:"iconSvg"`
}

var (
	wordBoundary = regexp.MustCompile(`([a-z0-9])([A-Z])`)
	acronymEnd   = regexp.MustCompile(`([A-Z])([A-Z][a-z])`)
	whitespace   = regexp.MustCompile(`\s+`)
)

func runReadme(args []string) error {
	flags := newFlags("readme")
	check := flags.Bool("check", false, "fail when the README or its icons are out of date instead of writing them")
	if err := flags.Parse(args); err != nil {
		return err
	}
	files, err := renderPluginReadme()
	if err != nil {
		return err
	}
	if *check {
		return checkReadmeFiles(files)
	}
	return writeReadmeFiles(files)
}

func pluginsDirectory() string {
	return repoPath("packages", "addons", "plugins")
}

func readmeIconsDirectory() string {
	return repoPath("packages", "assets", "readme-icons")
}

func readReadmePlugins() ([]readmePlugin, error) {
	entries, err := os.ReadDir(pluginsDirectory())
	if err != nil {
		return nil, err
	}
	var plugins []readmePlugin
	for _, entry := range entries {
		manifestPath := filepath.Join(pluginsDirectory(), entry.Name(), "manifest.toml")
		if !entry.IsDir() || !fileExists(manifestPath) {
			continue
		}
		data, err := os.ReadFile(manifestPath)
		if err != nil {
			return nil, err
		}
		plugin := readmePlugin{folder: entry.Name()}
		if err := toml.Unmarshal(data, &plugin); err != nil {
			return nil, fmt.Errorf("%s/manifest.toml: %w", entry.Name(), err)
		}
		plugins = append(plugins, plugin)
	}
	sort.Slice(plugins, func(first, second int) bool {
		return strings.ToLower(plugins[first].Name) < strings.ToLower(plugins[second].Name)
	})
	return plugins, nil
}

func readmeIconFile(plugin readmePlugin) string {
	if plugin.IconSVG != "" {
		return plugin.folder + ".svg"
	}
	name := acronymEnd.ReplaceAllString(wordBoundary.ReplaceAllString(plugin.Icon, "$1-$2"), "$1-$2")
	return strings.ToLower(strings.ReplaceAll(name, "_", "-")) + ".svg"
}

func readmeIcon(plugin readmePlugin) (string, error) {
	if plugin.IconSVG != "" {
		return strings.ReplaceAll(strings.TrimSpace(plugin.IconSVG), "currentColor", readmeGrey) + "\n", nil
	}
	png, err := os.ReadFile(repoPath("packages", "assets", "plugin-icons", plugin.Icon+".png"))
	if err != nil {
		return "", fmt.Errorf("plugin icon %s for %s is missing from packages/assets/plugin-icons: %w", plugin.Icon, plugin.folder, err)
	}
	return strings.Join([]string{
		fmt.Sprintf(`<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 24 24">`, readmeIconSize, readmeIconSize),
		`<filter id="grey"><feColorMatrix values="0 0 0 0 0.55 0 0 0 0 0.55 0 0 0 0 0.58 0 0 0 1 0"/></filter>`,
		`<image width="24" height="24" filter="url(#grey)" href="data:image/png;base64,` + base64.StdEncoding.EncodeToString(png) + `"/>`,
		"</svg>",
		"",
	}, "\n"), nil
}

func oneLine(value string) string {
	return strings.TrimSpace(whitespace.ReplaceAllString(value, " "))
}

func renderPluginReadme() (map[string]string, error) {
	plugins, err := readReadmePlugins()
	if err != nil {
		return nil, err
	}
	files := map[string]string{}
	iconsPath, err := filepath.Rel(pluginsDirectory(), readmeIconsDirectory())
	if err != nil {
		return nil, err
	}
	lines := []string{
		"# Plugins",
		"",
		fmt.Sprintf("%d first-party plugins for astral, each in its own folder here.", len(plugins)),
		"",
		"<!-- Generated from each manifest.toml by `astral readme`. Do not edit by hand. -->",
		"",
	}
	for _, plugin := range plugins {
		icon, err := readmeIcon(plugin)
		if err != nil {
			return nil, err
		}
		file := readmeIconFile(plugin)
		files[filepath.Join(readmeIconsDirectory(), file)] = icon
		lines = append(lines, fmt.Sprintf(`- <img src="%s/%s" width="%d" height="%d" align="absmiddle" alt=""> [%s](%s/): %s`,
			filepath.ToSlash(iconsPath), file, readmeIconSize, readmeIconSize, oneLine(plugin.Name), plugin.folder, oneLine(plugin.Description)))
	}
	files[filepath.Join(pluginsDirectory(), "README.md")] = strings.Join(append(lines, ""), "\n")
	return files, nil
}

func staleReadmeIcons(files map[string]string) ([]string, error) {
	entries, err := os.ReadDir(readmeIconsDirectory())
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var stale []string
	for _, entry := range entries {
		path := filepath.Join(readmeIconsDirectory(), entry.Name())
		if _, wanted := files[path]; !wanted {
			stale = append(stale, path)
		}
	}
	return stale, nil
}

func writeReadmeFiles(files map[string]string) error {
	if err := os.MkdirAll(readmeIconsDirectory(), 0o755); err != nil {
		return err
	}
	for path, contents := range files {
		current, err := os.ReadFile(path)
		if err == nil && bytes.Equal(current, []byte(contents)) {
			continue
		}
		if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
			return err
		}
	}
	stale, err := staleReadmeIcons(files)
	if err != nil {
		return err
	}
	for _, path := range stale {
		if err := os.Remove(path); err != nil {
			return err
		}
	}
	return nil
}

func checkReadmeFiles(files map[string]string) error {
	var outdated []string
	for path, contents := range files {
		current, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(current, []byte(contents)) {
			outdated = append(outdated, repoRelative(path))
		}
	}
	stale, err := staleReadmeIcons(files)
	if err != nil {
		return err
	}
	for _, path := range stale {
		outdated = append(outdated, repoRelative(path))
	}
	if len(outdated) == 0 {
		return nil
	}
	sort.Strings(outdated)
	return fmt.Errorf("the plugin README is out of date; run astral readme (%s)", strings.Join(outdated, ", "))
}

func repoRelative(path string) string {
	relative, err := filepath.Rel(repoPath(), path)
	if err != nil {
		return path
	}
	return relative
}

func testReadme(string) error {
	files, err := renderPluginReadme()
	if err != nil {
		return err
	}
	return checkReadmeFiles(files)
}
