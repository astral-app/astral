package main

import (
	"errors"
	"os"
	"os/exec"
	"strings"
)

var luaPaths = []string{"packages/addons/plugins", "packages/core/lua", "packages/core/tests", "packages/sdk"}

var nativePatterns = []string{"packages/*.c", "packages/*.h", "packages/*.m", "packages/*.mm", "packages/*.cpp"}

func taplo(args ...string) error {
	return runWithEnv(repoPath(), []string{"RUST_LOG=warn"}, "taplo", args...)
}

func clangFormat(args ...string) error {
	files, err := trackedFiles(nativePatterns...)
	if err != nil || len(files) == 0 {
		return err
	}
	return run(repoPath(), "clang-format", append(args, files...)...)
}

func goFiles() ([]string, error) {
	return trackedFiles("packages/tools/*.go")
}

func checkGoFormat() error {
	files, err := goFiles()
	if err != nil || len(files) == 0 {
		return err
	}
	unformatted, err := output(repoPath(), "gofmt", append([]string{"-l"}, files...)...)
	if err != nil {
		return err
	}
	if strings.TrimSpace(unformatted) != "" {
		return errors.New("gofmt would change:\n" + unformatted)
	}
	return nil
}

func typecheckLua() error {
	logs, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	command := exec.Command("lua-language-server", "--check="+repoPath(), "--configpath="+repoPath(".luarc.json"), "--checklevel=Warning", "--logpath="+logs)
	result, _ := command.CombinedOutput()
	text := strings.ReplaceAll(string(result), "\r", "\n")
	if strings.Contains(text, "no problems found") {
		return nil
	}
	var findings []string
	for _, line := range strings.Split(text, "\n") {
		if strings.Contains(line, "[Warning]") || strings.Contains(line, "[Error]") {
			findings = append(findings, strings.TrimSpace(line))
		}
	}
	if len(findings) == 0 {
		findings = []string{strings.TrimSpace(text)}
	}
	return errors.New("lua-language-server:\n" + strings.Join(findings, "\n"))
}

func runLint(args []string) error {
	steps := []func() error{
		func() error { return run(repoPath(), "stylua", append([]string{"--check"}, luaPaths...)...) },
		func() error { return taplo("lint") },
		func() error { return taplo("format", "--check") },
		func() error {
			return run(repoPath(), "luacheck", "--quiet", "packages/addons/plugins", "packages/core/lua", "packages/core/tests")
		},
		typecheckLua,
		func() error { return clangFormat("--dry-run", "-Werror") },
		checkGoFormat,
		func() error { return run(repoPath("packages", "tools"), "go", "vet", "./...") },
		lintAllWorkflows,
	}
	for _, step := range steps {
		if err := step(); err != nil {
			return err
		}
	}
	return nil
}

func runFormat(args []string) error {
	if err := run(repoPath(), "stylua", luaPaths...); err != nil {
		return err
	}
	if err := taplo("format"); err != nil {
		return err
	}
	if err := clangFormat("-i"); err != nil {
		return err
	}
	if err := formatAllYAML(); err != nil {
		return err
	}
	files, err := goFiles()
	if err != nil || len(files) == 0 {
		return err
	}
	return run(repoPath(), "gofmt", append([]string{"-w"}, files...)...)
}

func fileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
