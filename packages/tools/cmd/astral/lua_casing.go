package main

import (
	"fmt"
	"os"
	"regexp"
	"strings"
)

var (
	snakeCase       = regexp.MustCompile(`^_?[a-z][a-z0-9_]*$|^_$`)
	upperSnakeCase  = regexp.MustCompile(`^[A-Z][A-Z0-9_]*$`)
	pascalCase      = regexp.MustCompile(`^[A-Z][A-Za-z0-9]*$`)
	localNames      = regexp.MustCompile(`\blocal\s+(function\s+)?([A-Za-z_][\w]*(?:\s*<\w+>)?(?:\s*,\s*[A-Za-z_][\w]*(?:\s*<\w+>)?)*)(\s*=\s*([\w.]+)(\s*\()?)?`)
	functionNames   = regexp.MustCompile(`\bfunction\s+([\w.:]+)\s*\(([^)]*)\)`)
	anonymousParams = regexp.MustCompile(`\bfunction\s*\(([^)]*)\)`)
	loopNames       = regexp.MustCompile(`\bfor\s+([\w\s,]+?)\s+(?:in\b|=)`)
	typeFactories   = map[string]bool{"astral.class": true, "astral.enum": true, "setmetatable": true}
)

type casingFinding struct {
	line int
	name string
	kind string
}

func stripLuaText(source string) string {
	var out strings.Builder
	for index := 0; index < len(source); {
		switch {
		case strings.HasPrefix(source[index:], "--[["):
			end := strings.Index(source[index:], "]]")
			if end < 0 {
				end = len(source) - index - 2
			}
			out.WriteString(blankKeepingLines(source[index : index+end+2]))
			index += end + 2
		case strings.HasPrefix(source[index:], "--"):
			end := strings.IndexByte(source[index:], '\n')
			if end < 0 {
				end = len(source) - index
			}
			index += end
		case strings.HasPrefix(source[index:], "[["):
			end := strings.Index(source[index+2:], "]]")
			if end < 0 {
				end = len(source) - index - 4
			}
			out.WriteString(`""` + blankKeepingLines(source[index:index+end+4]))
			index += end + 4
		case source[index] == '"' || source[index] == '\'':
			quote := source[index]
			end := index + 1
			for end < len(source) && source[end] != quote && source[end] != '\n' {
				if source[end] == '\\' {
					end++
				}
				end++
			}
			out.WriteString(`""`)
			index = end + 1
		default:
			out.WriteByte(source[index])
			index++
		}
	}
	return out.String()
}

func blankKeepingLines(text string) string {
	return strings.Repeat("\n", strings.Count(text, "\n"))
}

func lineAt(source string, offset int) int {
	return strings.Count(source[:offset], "\n") + 1
}

func splitNames(list string) []string {
	var names []string
	for _, part := range strings.Split(list, ",") {
		name := strings.TrimSpace(part)
		if bracket := strings.IndexByte(name, '<'); bracket >= 0 {
			name = strings.TrimSpace(name[:bracket])
		}
		if name != "" && name != "..." && name != "self" {
			names = append(names, name)
		}
	}
	return names
}

func valueName(name string) bool {
	return snakeCase.MatchString(name) || upperSnakeCase.MatchString(name)
}

func luaCasingFindings(source string) []casingFinding {
	code := stripLuaText(source)
	var findings []casingFinding
	add := func(offset int, name, kind string) {
		findings = append(findings, casingFinding{lineAt(code, offset), name, kind})
	}
	for _, match := range localNames.FindAllStringSubmatchIndex(code, -1) {
		names := splitNames(code[match[4]:match[5]])
		typed := false
		if match[8] >= 0 {
			value := code[match[8]:match[9]]
			segments := strings.Split(value, ".")
			typed = (match[10] >= 0 && typeFactories[value]) || (match[10] < 0 && pascalCase.MatchString(segments[len(segments)-1]))
		}
		for _, name := range names {
			class := pascalCase.MatchString(name) && strings.Contains(code, name+".__index")
			if valueName(name) || class || (typed && pascalCase.MatchString(name)) {
				continue
			}
			add(match[0], name, "local")
		}
	}
	for _, match := range functionNames.FindAllStringSubmatchIndex(code, -1) {
		path := code[match[2]:match[3]]
		parts := strings.FieldsFunc(path, func(r rune) bool { return r == '.' || r == ':' })
		if last := parts[len(parts)-1]; !snakeCase.MatchString(last) {
			add(match[0], last, "function")
		}
		for _, name := range splitNames(code[match[4]:match[5]]) {
			if !snakeCase.MatchString(name) {
				add(match[0], name, "parameter")
			}
		}
	}
	for _, match := range anonymousParams.FindAllStringSubmatchIndex(code, -1) {
		for _, name := range splitNames(code[match[2]:match[3]]) {
			if !snakeCase.MatchString(name) {
				add(match[0], name, "parameter")
			}
		}
	}
	for _, match := range loopNames.FindAllStringSubmatchIndex(code, -1) {
		for _, name := range splitNames(code[match[2]:match[3]]) {
			if !snakeCase.MatchString(name) {
				add(match[0], name, "loop variable")
			}
		}
	}
	return findings
}

func checkLuaCasing(files []string) error {
	var problems []string
	for _, file := range files {
		source, err := os.ReadFile(repoPath(file))
		if err != nil {
			return err
		}
		for _, finding := range luaCasingFindings(string(source)) {
			problems = append(problems, fmt.Sprintf("%s:%d: %s %q should be snake_case (UPPER_SNAKE for constants, PascalCase only for classes and enums)", file, finding.line, finding.kind, finding.name))
		}
	}
	if len(problems) > 0 {
		return fmt.Errorf("lua casing:\n%s", strings.Join(problems, "\n"))
	}
	return nil
}
