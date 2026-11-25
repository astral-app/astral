package main

import (
	"fmt"
	"os"
	"strings"
)

type spacedBlock struct {
	childIndent int
	seen        bool
	isItem      bool
}

func indentOf(line string) int {
	return len(line) - len(strings.TrimLeft(line, " "))
}

func blockOpener(trimmed string) (bool, bool) {
	switch trimmed {
	case "steps:":
		return true, true
	case "jobs:":
		return true, false
	}
	return false, false
}

func isBlockChild(line string, block spacedBlock) bool {
	trimmed := strings.TrimSpace(line)
	if indentOf(line) != block.childIndent || strings.HasPrefix(trimmed, "#") {
		return false
	}
	if block.isItem {
		return strings.HasPrefix(trimmed, "- ")
	}
	return strings.HasSuffix(trimmed, ":") || strings.Contains(trimmed, ": ")
}

func withBlankBeforeComments(out []string) []string {
	at := len(out)
	for at > 0 && strings.HasPrefix(strings.TrimSpace(out[at-1]), "#") {
		at--
	}
	if at == 0 || strings.TrimSpace(out[at-1]) == "" {
		return out
	}
	spaced := append([]string{}, out[:at]...)
	spaced = append(spaced, "")
	return append(spaced, out[at:]...)
}

func spaceWorkflow(text string) string {
	lines := strings.Split(text, "\n")
	var out []string
	var blocks []spacedBlock
	pending := -1
	pendingItem := false
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" {
			out = append(out, line)
			continue
		}
		indent := indentOf(line)
		if pending >= 0 {
			if indent > pending {
				blocks = append(blocks, spacedBlock{childIndent: indent, isItem: pendingItem})
			}
			pending = -1
		}
		for len(blocks) > 0 && indent < blocks[len(blocks)-1].childIndent {
			blocks = blocks[:len(blocks)-1]
		}
		if len(blocks) > 0 && isBlockChild(line, blocks[len(blocks)-1]) {
			block := &blocks[len(blocks)-1]
			if block.seen {
				out = withBlankBeforeComments(out)
			}
			block.seen = true
		}
		if opens, item := blockOpener(trimmed); opens {
			pending = indent
			pendingItem = item
		}
		out = append(out, line)
	}
	return strings.Join(out, "\n")
}

func spaceWorkflows(files []string, check bool) error {
	for _, file := range files {
		if !strings.HasPrefix(file, ".github/workflows/") {
			continue
		}
		path := repoPath(file)
		data, err := os.ReadFile(path)
		if err != nil {
			return fmt.Errorf("failed to read workflow %s: %w", file, err)
		}
		spaced := spaceWorkflow(string(data))
		if spaced == string(data) {
			continue
		}
		if check {
			return fmt.Errorf("%s: separate each job and step with a blank line (run astral format)", file)
		}
		if err := os.WriteFile(path, []byte(spaced), 0o644); err != nil {
			return fmt.Errorf("failed to write workflow %s: %w", file, err)
		}
	}
	return nil
}
