package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

type catalogPlugin struct {
	ID          string `toml:"id"`
	Name        string `toml:"name"`
	Version     string `toml:"version"`
	Description string `toml:"description"`
	Icon        string `toml:"icon"`
}

type catalogIndex struct {
	Plugins []catalogPlugin `toml:"plugins"`
}

type manifestListing struct {
	ID          string `toml:"id"`
	Name        string `toml:"name"`
	Version     string `toml:"version"`
	Description string `toml:"description"`
	Icon        string `toml:"icon"`
}

func catalogDrift() ([]string, error) {
	data, err := os.ReadFile(repoPath("packages", "catalog", "index.toml"))
	if err != nil {
		return nil, err
	}
	var index catalogIndex
	if err := toml.Unmarshal(data, &index); err != nil {
		return nil, fmt.Errorf("packages/catalog/index.toml: %w", err)
	}
	listed := map[string]catalogPlugin{}
	for _, plugin := range index.Plugins {
		listed[plugin.ID] = plugin
	}
	plugins, err := readReadmePlugins()
	if err != nil {
		return nil, err
	}
	var drift []string
	for _, plugin := range plugins {
		manifestData, err := os.ReadFile(repoPath("packages", "addons", "plugins", plugin.folder, "manifest.toml"))
		if err != nil {
			return nil, err
		}
		var manifest manifestListing
		if err := toml.Unmarshal(manifestData, &manifest); err != nil {
			return nil, err
		}
		entry, found := listed[manifest.ID]
		if !found {
			continue
		}
		wantIcon := manifest.Icon
		if !strings.HasSuffix(wantIcon, "Icon") {
			wantIcon = ""
		}
		for _, field := range []struct{ name, catalog, manifest string }{
			{"name", entry.Name, manifest.Name},
			{"version", entry.Version, manifest.Version},
			{"description", entry.Description, manifest.Description},
			{"icon", entry.Icon, wantIcon},
		} {
			if field.catalog != field.manifest {
				drift = append(drift, fmt.Sprintf("%s %s: catalog %q, manifest %q", manifest.ID, field.name, field.catalog, field.manifest))
			}
		}
	}
	return drift, nil
}

func testCatalog(string) error {
	drift, err := catalogDrift()
	if err != nil {
		return err
	}
	if len(drift) > 0 {
		return fmt.Errorf("packages/catalog/index.toml disagrees with the plugin manifests:\n  %s", strings.Join(drift, "\n  "))
	}
	return nil
}
