package main

import (
	"errors"
	"fmt"
	"os"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

type semanticEntry map[string][2]string

var (
	semanticKeysPattern  = regexp.MustCompile(`r\d+ = \{('[A-Z0-9_]+': null(?:, )?)+\};`)
	semanticKeyPattern   = regexp.MustCompile(`'([A-Z0-9_]+)': null`)
	semanticThemePattern = regexp.MustCompile(`= r\d+\.(ASH|LIGHT|ONYX|DARK|DARKER|MIDNIGHT);`)
	semanticRawPattern   = regexp.MustCompile(`\{'raw': '([A-Z0-9_]+)', 'opacity': ([0-9.]+)\}`)
	semanticIndexPattern = regexp.MustCompile(`^(r\d+)\[(\d+)\] = (r\d+);$`)
	semanticEntryPattern = regexp.MustCompile(`^(r\d+) = \{'category':`)
)

func parseSemanticColors(source string) ([]string, map[int]semanticEntry, error) {
	start := strings.Index(source, "'SemanticColors': null")
	if start < 0 {
		return nil, nil, errors.New("no SemanticColors in the source")
	}
	keysMatch := semanticKeysPattern.FindStringIndex(source[start:])
	if keysMatch == nil {
		return nil, nil, errors.New("no token list after SemanticColors")
	}
	keysText := source[start+keysMatch[0] : start+keysMatch[1]]
	var keys []string
	for _, match := range semanticKeyPattern.FindAllStringSubmatch(keysText, -1) {
		keys = append(keys, match[1])
	}
	entries := map[int]semanticEntry{}
	current := semanticEntry{}
	theme := ""
	entryRegister := ""
	for _, raw := range strings.Split(source[start+keysMatch[1]:], "\n") {
		line := strings.TrimSpace(raw)
		if match := semanticEntryPattern.FindStringSubmatch(line); match != nil {
			current = semanticEntry{}
			entryRegister = match[1]
			continue
		}
		if match := semanticThemePattern.FindStringSubmatch(line); match != nil {
			theme = strings.ToLower(match[1])
			continue
		}
		if match := semanticRawPattern.FindStringSubmatch(line); match != nil && theme != "" {
			current[theme] = [2]string{match[1], match[2]}
			theme = ""
			continue
		}
		if match := semanticIndexPattern.FindStringSubmatch(line); match != nil && match[3] == entryRegister && match[1] != entryRegister && len(current) > 0 {
			index, _ := strconv.Atoi(match[2])
			if index >= len(keys) {
				break
			}
			entries[index] = current
			current = semanticEntry{}
			if len(entries) == len(keys) {
				break
			}
		}
	}
	if len(entries) == 0 {
		return nil, nil, errors.New("no semantic colour entries found")
	}
	return keys, entries, nil
}

func renderSemanticPalette(keys []string, entries map[int]semanticEntry, origin string) string {
	var output strings.Builder
	fmt.Fprintf(&output, "name = \"Discord semantic colours\"\nsource = %q\n\n[colors]\n", origin)
	indices := make([]int, 0, len(entries))
	for index := range entries {
		indices = append(indices, index)
	}
	sort.Slice(indices, func(left, right int) bool { return keys[indices[left]] < keys[indices[right]] })
	for _, index := range indices {
		entry := entries[index]
		themes := make([]string, 0, len(entry))
		for theme := range entry {
			themes = append(themes, theme)
		}
		sort.Strings(themes)
		parts := make([]string, 0, len(themes))
		for _, theme := range themes {
			parts = append(parts, fmt.Sprintf("%s = [%q, %s]", theme, entry[theme][0], entry[theme][1]))
		}
		fmt.Fprintf(&output, "%s = { %s }\n", keys[index], strings.Join(parts, ", "))
	}
	return output.String()
}

func runPalette(args []string) error {
	if len(args) == 0 || args[0] != "semantic" {
		return errors.New("usage: astral palette semantic --source <generated-definitions.tsx> --origin <description> [--output <toml>]")
	}
	args = args[1:]
	flags := newFlags("palette")
	source := flags.String("source", "", "decompiled generated-definitions.tsx from Discord's tokens package")
	origin := flags.String("origin", "", "where the definitions came from, such as \"Discord 355.0 generated-definitions\"")
	destination := flags.String("output", repoPath("packages", "addons", "themes", "palettes", "discord-semantic.toml"), "where to write the palette")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *source == "" || *origin == "" {
		return errors.New("usage: astral palette semantic --source <generated-definitions.tsx> --origin <description> [--output <toml>]")
	}
	text, err := os.ReadFile(*source)
	if err != nil {
		return err
	}
	keys, entries, err := parseSemanticColors(string(text))
	if err != nil {
		return err
	}
	if err := os.WriteFile(*destination, []byte(renderSemanticPalette(keys, entries, *origin)), 0o644); err != nil {
		return err
	}
	fmt.Printf("wrote %d of %d semantic colours to %s\n", len(entries), len(keys), *destination)
	return nil
}
