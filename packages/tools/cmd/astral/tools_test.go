package main

import (
	"crypto/ed25519"
	"crypto/rand"
	"debug/macho"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/pelletier/go-toml/v2"
	"howett.net/plist"
)

func writePlugin(t *testing.T) string {
	t.Helper()
	folder := t.TempDir()
	files := map[string]string{
		"manifest.toml":    "id = \"astral.sample\"\nname = \"Sample\"\nmain = \"main.lua\"\npermissions = [\"storage\"]\n",
		"main.lua":         "local util = require(\"lib.util\")\n",
		"lib/util.lua":     "return {}\n",
		"i18n/en-US.toml":  "HELLO = \"Hello\"\n",
		"tests/sample.lua": "print(\"ok\")\n",
	}
	if err := writeFiles(folder, files); err != nil {
		t.Fatal(err)
	}
	return folder
}

func TestSignedPathsCoverModulesButNotTests(t *testing.T) {
	paths, err := signedPaths(writePlugin(t))
	if err != nil {
		t.Fatal(err)
	}
	got := strings.Join(paths, ",")
	if got != "i18n/en-US.toml,lib/util.lua,lib/util.luac,main.lua,main.luac,manifest.toml" {
		t.Fatalf("signed paths %s", got)
	}
}

func TestCompilePluginWritesBytecodeWithChunkNames(t *testing.T) {
	folder := writePlugin(t)
	if err := compilePlugin(folder); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"main.luac", "lib/util.luac"} {
		data, err := os.ReadFile(filepath.Join(folder, path))
		if err != nil || len(data) < 4 || string(data[:4]) != "\x1bLua" {
			t.Fatalf("%s is not Lua bytecode", path)
		}
	}
	util, _ := os.ReadFile(filepath.Join(folder, "lib", "util.luac"))
	if !strings.Contains(string(util), "astral.sample/lib.util") {
		t.Fatal("module bytecode lacks its chunk name")
	}
	if _, err := os.Stat(filepath.Join(folder, "tests", "sample.luac")); err == nil {
		t.Fatal("tests were compiled")
	}
}

func TestSignatureRoundTripAndTamper(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	folder := writePlugin(t)
	if err := writePluginSignature(folder, private); err != nil {
		t.Fatal(err)
	}
	if err := verifyPlugin(folder, public); err != nil {
		t.Fatalf("fresh signature rejected: %v", err)
	}
	if err := os.WriteFile(filepath.Join(folder, "lib", "util.lua"), []byte("return { changed = true }\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := verifyPlugin(folder, public); err == nil {
		t.Fatal("a changed module still verified")
	}
	if err := writePluginSignature(folder, private); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(folder, "lib", "extra.lua"), []byte("return 1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := verifyPlugin(folder, public); err == nil {
		t.Fatal("an unsigned module still verified")
	}
	other, _, _ := ed25519.GenerateKey(rand.Reader)
	os.Remove(filepath.Join(folder, "lib", "extra.lua"))
	if err := verifyPlugin(folder, other); err == nil {
		t.Fatal("a signature verified against the wrong key")
	}
}

func TestSignatureMessageIsSorted(t *testing.T) {
	message := string(signatureMessage(map[string]string{"main.lua": "b", "manifest.toml": "a", "i18n/x.toml": "c"}))
	want := signatureVersion + "\ni18n/x.toml c\nmain.lua b\nmanifest.toml a\n"
	if message != want {
		t.Fatalf("message %q", message)
	}
}

func TestStagePluginCopiesSignedFilesOnly(t *testing.T) {
	_, private, _ := ed25519.GenerateKey(rand.Reader)
	folder := writePlugin(t)
	if err := writePluginSignature(folder, private); err != nil {
		t.Fatal(err)
	}
	destination := t.TempDir()
	if err := stagePlugin(folder, destination); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"manifest.toml", "main.lua", "lib/util.lua", "i18n/en-US.toml", "signature"} {
		if !fileExists(filepath.Join(destination, path)) {
			t.Fatalf("%s was not staged", path)
		}
	}
	if fileExists(filepath.Join(destination, "tests")) {
		t.Fatal("tests were staged")
	}
}

func TestLoadedDylibsReadsLoadCommands(t *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	file, err := macho.Open(executable)
	if err != nil {
		t.Skip("not a Mach-O test binary")
	}
	defer file.Close()
	found := false
	for _, library := range loadedDylibs(file) {
		found = found || strings.HasPrefix(library, "/usr/lib/libSystem")
	}
	if !found {
		t.Fatalf("libSystem missing from %v", loadedDylibs(file))
	}
}

func TestCompatReportListsMovedProperties(t *testing.T) {
	baseline := &compatResult{Phoenix: true, Version: "356.0", Build: "1", PhoenixUUID: "a", Probe: probeResult{Properties: map[string]uint16{"text": 499, "uri": 542}}}
	current := compatResult{Phoenix: true, Version: "357.0", Build: "2", PhoenixUUID: "b", Supported: true, Probe: probeResult{Properties: map[string]uint16{"text": 501, "uri": 542}}}
	report := compatReport(current, baseline)
	if !strings.Contains(report, "`text` 499 → 501") || strings.Contains(report, "`uri`") {
		t.Fatalf("report %s", report)
	}
}

func TestScaffoldManifestIsValidTOML(t *testing.T) {
	var manifest pluginManifest
	if err := toml.Unmarshal([]byte(manifestTemplate("astral.sample", "Sample", "adrian")), &manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.ID != "astral.sample" || manifest.Main != "main.lua" || manifest.Permissions == nil {
		t.Fatalf("manifest %+v", manifest)
	}
}

func TestProfileCoversBundleAndDevice(t *testing.T) {
	profile := provisioningProfile{
		Expiration:   time.Now().Add(time.Hour),
		Devices:      []string{"00008030-ABC"},
		Team:         []string{"TEAM"},
		Entitlements: map[string]interface{}{"application-identifier": "TEAM.dev.example.*"},
	}
	if !profile.covers("dev.example.app", "00008030-abc") {
		t.Fatal("wildcard profile did not cover a matching bundle")
	}
	if profile.covers("com.other.app", "00008030-ABC") || profile.covers("dev.example.app", "OTHER") {
		t.Fatal("profile covered the wrong bundle or device")
	}
	profile.Expiration = time.Now().Add(-time.Hour)
	if profile.covers("dev.example.app", "00008030-ABC") {
		t.Fatal("expired profile still covered")
	}
}

func TestProfileFindsTheSignedApplicationGroup(t *testing.T) {
	profile := provisioningProfile{
		Expiration: time.Now().Add(time.Hour),
		Devices:    []string{"00008030-ABC"},
		Team:       []string{"TEAM"},
		Entitlements: map[string]interface{}{
			"application-identifier":                "TEAM.dev.example.*",
			"com.apple.security.application-groups": []interface{}{"", "group.dev.example.discord", "group.dev.example.other"},
		},
	}
	if profile.firstApplicationGroup() != "group.dev.example.discord" {
		t.Fatalf("application group %q", profile.firstApplicationGroup())
	}
	selected, err := profileForExtension(profile, "dev.example.discord.Share", "00008030-abc")
	if err != nil || selected.firstApplicationGroup() != profile.firstApplicationGroup() {
		t.Fatalf("extension profile did not preserve the shared group: %v", err)
	}
}

func TestParseSemanticColors(t *testing.T) {
	source := `r1 = {'SemanticColors': null};
        r5 = {'TEXT_DEFAULT': null, 'BACKGROUND_BASE_LOW': null};
        r6 = {'category': 'generic'};
        r9 = r7.DARK;
        r8 = {'raw': 'NEUTRAL_5', 'opacity': 1};
        r6[9] = r8;
        r9 = r7.LIGHT;
        r8 = {'raw': 'NEUTRAL_72', 'opacity': 0.5};
        r6[9] = r8;
        r5[0] = r6;
        r6 = {'category': 'generic'};
        r9 = r7.DARK;
        r8 = {'raw': 'NEUTRAL_82', 'opacity': 1};
        r6[9] = r8;
        r5[1] = r6;`
	keys, entries, err := parseSemanticColors(source)
	if err != nil {
		t.Fatal(err)
	}
	if len(keys) != 2 || len(entries) != 2 || entries[0]["dark"] != [2]string{"NEUTRAL_5", "1"} || entries[0]["light"] != [2]string{"NEUTRAL_72", "0.5"} || entries[1]["dark"] != [2]string{"NEUTRAL_82", "1"} {
		t.Fatalf("keys %v entries %v", keys, entries)
	}
	rendered := renderSemanticPalette(keys, entries, "test")
	if !strings.Contains(rendered, `BACKGROUND_BASE_LOW = { dark = ["NEUTRAL_82", 1] }`) {
		t.Fatalf("rendered %s", rendered)
	}
}

func TestDoctorReadsCIPins(t *testing.T) {
	workflow, err := os.ReadFile(repoPath(".github", "workflows", "ci.yml"))
	if err != nil {
		t.Fatal(err)
	}
	pins := pinsFrom(string(workflow))
	for _, tool := range pinnedTools {
		if pins[tool.binary] == "" {
			t.Fatalf("no CI pin for %s", tool.binary)
		}
	}
}

func TestDoctorVersionMatching(t *testing.T) {
	cases := map[string]bool{
		"clang-format version 23.1.5": true,
		"clang-format version 23.1.6": false,
		"stylua 2.6.0":                true,
		"stylua 12.6.0":               false,
		"3.19.2":                      true,
		"3.19.20":                     false,
	}
	pins := map[string]string{"clang-format version 23.1.5": "23.1.5", "clang-format version 23.1.6": "23.1.5", "stylua 2.6.0": "2.6.0", "stylua 12.6.0": "2.6.0", "3.19.2": "3.19.2", "3.19.20": "3.19.2"}
	for output, want := range cases {
		if versionMatches(output, pins[output]) != want {
			t.Fatalf("versionMatches(%q, %q) != %v", output, pins[output], want)
		}
	}
}

func plannedNames(changed ...string) string {
	var names []string
	for _, step := range affectedChecks(checkRules(), changed) {
		names = append(names, step.rule.name)
	}
	return strings.Join(names, ",")
}

func TestCheckPicksAffectedSteps(t *testing.T) {
	cases := map[string]string{
		"packages/core/wire/PhoenixWire.h":   "clang-format,wire tests",
		"packages/core/LuaSandbox.cpp":       "clang-format,core tests",
		"packages/tools/cmd/astral/check.go": "gofmt,go vet and tests",
		"packages/ios/sources/Themes.mm":     "clang-format",
		"README.md":                          "",
	}
	for path, want := range cases {
		if got := plannedNames(path); got != want {
			t.Fatalf("%s: got %q, want %q", path, got, want)
		}
	}
}

func TestCheckSkipsFileScopedStepsForDeletions(t *testing.T) {
	if got := plannedNames("packages/core/Removed.cpp"); got != "core tests" {
		t.Fatalf("got %q", got)
	}
}

func TestAdhocPresetUnblocksPrecheckAdmission(t *testing.T) {
	source := map[string]any{"Identifier": "standard", "Selection": map[string]any{"Kind": "Block", "Patches": []any{"dyld-exp-mis_trust_auth", adhocAdmission}}}
	data, err := plist.Marshal(source, plist.XMLFormat)
	if err != nil {
		t.Fatal(err)
	}
	result, err := adhocPreset(data)
	if err != nil {
		t.Fatal(err)
	}
	var preset map[string]any
	if _, err := plist.Unmarshal(result, &preset); err != nil {
		t.Fatal(err)
	}
	patches := preset["Selection"].(map[string]any)["Patches"].([]any)
	if preset["Identifier"] != "standard-adhoc" || len(patches) != 1 || patches[0] != "dyld-exp-mis_trust_auth" {
		t.Fatalf("unexpected preset %v", preset)
	}
}
