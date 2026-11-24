package main

import (
	"os"
	"regexp"
	"testing"
)

func TestEveryPermissionHasADescription(t *testing.T) {
	host, err := os.ReadFile(repoPath("packages", "core", "src", "AstralLuaHost.cpp"))
	if err != nil {
		t.Fatal(err)
	}
	pages, err := os.ReadFile(repoPath("packages", "core", "src", "AstralSettingsPages.cpp"))
	if err != nil {
		t.Fatal(err)
	}
	described := map[string]bool{}
	for _, match := range regexp.MustCompile(`\{"([a-z_.]+)", \{"`).FindAllSubmatch(pages, -1) {
		described[string(match[1])] = true
	}
	required := regexp.MustCompile(`(?:RequirePermission\(state, |permission = )"([a-z_.]+)"`).FindAllSubmatch(host, -1)
	if len(required) == 0 {
		t.Fatal("found no permission checks in AstralLuaHost.cpp")
	}
	for _, match := range required {
		if !described[string(match[1])] {
			t.Errorf("permission %s has no description in AstralSettingsPages.cpp", match[1])
		}
	}
}
