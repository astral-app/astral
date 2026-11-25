package main

import (
	"errors"
	"fmt"
	"path/filepath"
	"strings"
)

type checkRule struct {
	name    string
	affects func(path string) bool
	run     func(files []string, work string) error
}

var nativeExtensions = map[string]bool{".c": true, ".h": true, ".m": true, ".mm": true, ".cpp": true}

func under(path string, prefixes ...string) bool {
	for _, prefix := range prefixes {
		if strings.HasPrefix(path, prefix) {
			return true
		}
	}
	return false
}

func isLua(path string) bool {
	return filepath.Ext(path) == ".lua" && under(path, "packages/")
}

func checkRules() []checkRule {
	return []checkRule{
		{"clang-format", func(path string) bool { return nativeExtensions[filepath.Ext(path)] && under(path, "packages/") },
			func(files []string, _ string) error {
				return run(repoPath(), "clang-format", append([]string{"--dry-run", "-Werror"}, files...)...)
			}},
		{"gofmt", func(path string) bool { return filepath.Ext(path) == ".go" && under(path, "packages/tools/") },
			func(files []string, _ string) error {
				unformatted, err := output(repoPath(), "gofmt", append([]string{"-l"}, files...)...)
				if err == nil && strings.TrimSpace(unformatted) != "" {
					err = errors.New("gofmt would change:\n" + unformatted)
				}
				return err
			}},
		{"go vet and tests", func(path string) bool { return under(path, "packages/tools/", ".github/workflows/ci.yml") },
			func(_ []string, work string) error {
				if err := run(repoPath("packages", "tools"), "go", "vet", "./..."); err != nil {
					return err
				}
				return testTools(work)
			}},
		{"stylua", isLua, func(files []string, _ string) error {
			return run(repoPath(), "stylua", append([]string{"--check"}, files...)...)
		}},
		{"luacheck", func(path string) bool {
			return isLua(path) && under(path, "packages/addons/plugins/", "packages/core/lua/", "packages/core/tests/")
		}, func(files []string, _ string) error {
			return run(repoPath(), "luacheck", append([]string{"--quiet"}, files...)...)
		}},
		{"lua casing", func(path string) bool {
			return isLua(path) && under(path, "packages/addons/plugins/", "packages/core/lua/", "packages/sdk/")
		}, func(files []string, _ string) error { return checkLuaCasing(files) }},
		{"lua types", func(path string) bool { return isLua(path) || path == ".luarc.json" },
			func(_ []string, _ string) error { return typecheckLua() }},
		{"taplo", func(path string) bool { return filepath.Ext(path) == ".toml" }, func(files []string, _ string) error {
			if err := taplo(append([]string{"lint"}, files...)...); err != nil {
				return err
			}
			return taplo(append([]string{"format", "--check"}, files...)...)
		}},
		{"workflows", isWorkflowYAML, func(files []string, _ string) error {
			var existing []string
			for _, file := range files {
				if file != ".yamlfmt.yml" && fileExists(repoPath(file)) {
					existing = append(existing, file)
				}
			}
			if len(existing) == 0 {
				return lintAllWorkflows()
			}
			return lintWorkflows(existing)
		}},
		{"wire tests", func(path string) bool {
			return under(path, "packages/core/wire/", "packages/ios/tests/")
		}, func(_ []string, work string) error { return testWire(work) }},
		{"core tests", func(path string) bool {
			return nativeExtensions[filepath.Ext(path)] && under(path, "packages/core/") && !under(path, "packages/core/wire/")
		}, func(_ []string, work string) error { return testCore(work) }},
		{"lua tests", func(path string) bool {
			return under(path, "packages/core/lua/", "packages/core/tests/", "packages/addons/plugins/") && !nativeExtensions[filepath.Ext(path)]
		}, func(_ []string, work string) error { return testLua(work) }},
		{"signatures", func(path string) bool { return under(path, "packages/addons/plugins/", "packages/catalog/", "keys/") },
			func(_ []string, work string) error { return testSignatures(work) }},
	}
}

func runCheck(args []string) error {
	flags := newFlags("check")
	staged := flags.Bool("staged", false, "check the files staged for commit")
	if err := flags.Parse(args); err != nil {
		return err
	}
	changed := flags.Args()
	if *staged {
		listing, err := output(repoPath(), "git", "diff", "--cached", "--name-only", "--diff-filter=ACMRD")
		if err != nil {
			return err
		}
		changed = strings.Fields(listing)
	}
	plan := affectedChecks(checkRules(), changed)
	if len(plan) == 0 {
		fmt.Println("check: nothing to check")
		return nil
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	for _, step := range plan {
		fmt.Println("check: " + step.rule.name)
		if err := step.rule.run(step.files, work); err != nil {
			return fmt.Errorf("%s: %w", step.rule.name, err)
		}
	}
	return nil
}

type plannedCheck struct {
	rule  checkRule
	files []string
}

func affectedChecks(rules []checkRule, changed []string) []plannedCheck {
	var plan []plannedCheck
	for _, rule := range rules {
		var files []string
		affected := false
		for _, path := range changed {
			if !rule.affects(path) {
				continue
			}
			affected = true
			if fileExists(repoPath(path)) {
				files = append(files, path)
			}
		}
		if affected && (len(files) > 0 || !fileScoped(rule.name)) {
			plan = append(plan, plannedCheck{rule, files})
		}
	}
	return plan
}

func fileScoped(name string) bool {
	return name == "clang-format" || name == "gofmt" || name == "stylua" || name == "luacheck" || name == "lua casing" || name == "taplo"
}
