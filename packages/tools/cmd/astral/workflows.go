package main

import (
	"path/filepath"
	"strings"
)

func isWorkflowYAML(path string) bool {
	extension := filepath.Ext(path)
	return (extension == ".yml" || extension == ".yaml") && under(path, ".github/") || path == ".yamlfmt.yml"
}

func yamlFiles() ([]string, error) {
	return trackedFiles(".github/*.yml", ".github/*.yaml")
}

func absolutePaths(files []string) []string {
	paths := make([]string, len(files))
	for index, file := range files {
		paths[index] = repoPath(file)
	}
	return paths
}

func goTool(name string, args ...string) error {
	return run(repoPath("packages", "tools"), "go", append([]string{"tool", name}, args...)...)
}

func yamlfmt(lint bool, files []string) error {
	args := []string{"-conf", repoPath(".yamlfmt.yml")}
	if lint {
		args = append(args, "-lint")
	}
	return goTool("yamlfmt", append(args, absolutePaths(files)...)...)
}

func lintWorkflows(files []string) error {
	var workflows []string
	for _, file := range files {
		if strings.HasPrefix(file, ".github/workflows/") {
			workflows = append(workflows, file)
		}
	}
	if len(workflows) > 0 {
		if err := goTool("actionlint", absolutePaths(workflows)...); err != nil {
			return err
		}
		if err := run(repoPath(), "zizmor", "--offline", "--quiet", ".github/workflows"); err != nil {
			return err
		}
	}
	return yamlfmt(true, files)
}

func lintAllWorkflows() error {
	files, err := yamlFiles()
	if err != nil || len(files) == 0 {
		return err
	}
	return lintWorkflows(files)
}

func formatAllYAML() error {
	files, err := yamlFiles()
	if err != nil || len(files) == 0 {
		return err
	}
	return yamlfmt(false, files)
}
