package main

import (
	"archive/zip"
	"bytes"
	"debug/macho"
	"encoding/binary"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"howett.net/plist"
)

func testMachO() []byte {
	data := make([]byte, 512)
	binary.LittleEndian.PutUint32(data[0:], uint32(macho.Magic64))
	binary.LittleEndian.PutUint32(data[4:], uint32(macho.CpuArm64))
	binary.LittleEndian.PutUint32(data[12:], uint32(macho.TypeBundle))
	return data
}

func testDylib() []byte {
	command := alignedDylibCommand(groupRemapInstall)
	binary.LittleEndian.PutUint32(command, dylibIDLoadCommand)
	data := make([]byte, machOHeaderSize64+len(command))
	binary.LittleEndian.PutUint32(data[0:], uint32(macho.Magic64))
	binary.LittleEndian.PutUint32(data[4:], uint32(macho.CpuArm64))
	binary.LittleEndian.PutUint32(data[12:], uint32(macho.TypeDylib))
	binary.LittleEndian.PutUint32(data[16:], 1)
	binary.LittleEndian.PutUint32(data[20:], uint32(len(command)))
	copy(data[machOHeaderSize64:], command)
	return data
}

func testPlist(t *testing.T, values map[string]interface{}) []byte {
	t.Helper()
	data, err := plist.Marshal(values, plist.XMLFormat)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func testIPA(t *testing.T, entries map[string][]byte) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "Discord.ipa")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(file)
	for name, data := range entries {
		header := &zip.FileHeader{Name: name, Method: zip.Deflate, Modified: time.Unix(1, 0)}
		header.SetMode(0o755)
		entry, err := writer.CreateHeader(header)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write(data); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	return path
}

func readIPAEntries(t *testing.T, path string) map[string][]byte {
	t.Helper()
	archive, err := zip.OpenReader(path)
	if err != nil {
		t.Fatal(err)
	}
	defer archive.Close()
	entries := map[string][]byte{}
	for _, file := range archive.File {
		data, err := readZipFile(file)
		if err != nil {
			t.Fatal(err)
		}
		entries[file.Name] = data
	}
	return entries
}

func extensionInfo(t *testing.T, identifier string, executable string) []byte {
	return testPlist(t, map[string]interface{}{
		"CFBundleIdentifier": identifier,
		"CFBundleExecutable": executable,
	})
}

func TestAddWeakDylib(t *testing.T) {
	original := testMachO()
	patched, err := addWeakDylibBytes(original, groupRemapInstall)
	if err != nil {
		t.Fatal(err)
	}
	file, err := macho.NewFile(bytes.NewReader(patched))
	if err != nil {
		t.Fatal(err)
	}
	if !machoLoadsDylib(file, groupRemapInstall) {
		t.Fatal("weak dylib load command is missing")
	}
	second, err := addWeakDylibBytes(patched, groupRemapInstall)
	if err != nil || !bytes.Equal(second, patched) {
		t.Fatalf("existing load command was not idempotent: %v", err)
	}
	if _, err := addWeakDylibBytes(original[:machOHeaderSize64], groupRemapInstall); err == nil {
		t.Fatal("load command was added without header padding")
	}
}

func TestAddGroupRemapPreservesAndWiresDiscordExtensions(t *testing.T) {
	app := "Payload/Discord.app/"
	entries := map[string][]byte{
		app + "Info.plist": testPlist(t, map[string]interface{}{
			"CFBundleIdentifier": "com.hammerandchisel.discord",
			"CFBundleExecutable": "Discord",
		}),
		app + "Discord":                                               testMachO(),
		app + "PlugIns/Share.appex/Info.plist":                        extensionInfo(t, "com.hammerandchisel.discord.Share", "Share"),
		app + "PlugIns/Share.appex/Share":                             testMachO(),
		app + "PlugIns/BroadcastUpload.appex/Info.plist":              extensionInfo(t, "com.hammerandchisel.discord.BroadcastUpload", "BroadcastUpload"),
		app + "PlugIns/BroadcastUpload.appex/BroadcastUpload":         testMachO(),
		app + "PlugIns/NotificationService.appex/Info.plist":          extensionInfo(t, "com.hammerandchisel.discord.NotificationService", "NotificationService"),
		app + "PlugIns/NotificationService.appex/NotificationService": testMachO(),
		app + "PlugIns/OpenInAstral.appex/Info.plist":                 extensionInfo(t, "com.hammerandchisel.discord.OpenInAstral", "OpenInAstral"),
		app + "PlugIns/OpenInAstral.appex/OpenInAstral":               testMachO(),
		app + "PlugIns/OpenInAstral.appex/manifest.json":              []byte("{\"manifest_version\":3}"),
	}
	path := testIPA(t, entries)
	library := filepath.Join(t.TempDir(), "AstralGroups.dylib")
	if err := os.WriteFile(library, testDylib(), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := addGroupRemapToIPA(path, library); err != nil {
		t.Fatal(err)
	}
	patched := readIPAEntries(t, path)
	if err := validateGroupRemapLibrary(patched[app+groupRemapFramework]); err != nil {
		t.Fatalf("group remap library was not embedded correctly: %v", err)
	}
	for _, path := range []string{
		app + "PlugIns/Share.appex/Share",
		app + "PlugIns/BroadcastUpload.appex/BroadcastUpload",
		app + "PlugIns/NotificationService.appex/NotificationService",
	} {
		file, err := macho.NewFile(bytes.NewReader(patched[path]))
		if err != nil || !machoLoadsDylib(file, groupRemapInstall) {
			t.Fatalf("%s does not load the group remap library: %v", path, err)
		}
	}
	own, err := macho.NewFile(bytes.NewReader(patched[app+"PlugIns/OpenInAstral.appex/OpenInAstral"]))
	if err != nil || machoLoadsDylib(own, groupRemapInstall) {
		t.Fatalf("astral's extension was patched as a Discord extension: %v", err)
	}
	if err := validateIPAGroupWiring(path); err != nil {
		t.Fatal(err)
	}
}

func TestAddOpenInAstralToIPAReplacesTheExistingExtension(t *testing.T) {
	app := "Payload/Discord.app/"
	path := testIPA(t, map[string][]byte{
		app + "Info.plist": testPlist(t, map[string]interface{}{
			"CFBundleIdentifier": "com.hammerandchisel.discord",
		}),
		app + "PlugIns/OpenInAstral.appex/old": []byte("old"),
	})
	extension := filepath.Join(t.TempDir(), "OpenInAstral.appex")
	if err := os.MkdirAll(extension, 0o755); err != nil {
		t.Fatal(err)
	}
	for name, data := range map[string][]byte{
		"Info.plist":    extensionInfo(t, "com.hammerandchisel.discord.OpenInAstral", "OpenInAstral"),
		"OpenInAstral":  testMachO(),
		"manifest.json": []byte("{\"manifest_version\":3}"),
	} {
		if err := os.WriteFile(filepath.Join(extension, name), data, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := addOpenInAstralToIPA(path, extension); err != nil {
		t.Fatal(err)
	}
	entries := readIPAEntries(t, path)
	if _, exists := entries[app+"PlugIns/OpenInAstral.appex/old"]; exists {
		t.Fatal("old OpenInAstral extension was not replaced")
	}
	if string(entries[app+"PlugIns/OpenInAstral.appex/manifest.json"]) != "{\"manifest_version\":3}" {
		t.Fatal("new Safari extension resources were not copied")
	}
}

func TestAddURLSchemeToIPAIsIdempotent(t *testing.T) {
	app := "Payload/Discord.app/"
	path := testIPA(t, map[string][]byte{
		app + "Info.plist": testPlist(t, map[string]interface{}{
			"CFBundleIdentifier": "com.hammerandchisel.discord",
			"CFBundleURLTypes": []interface{}{
				map[string]interface{}{
					"CFBundleURLName":    "com.hammerandchisel.discord",
					"CFBundleURLSchemes": []interface{}{"discord", "com.hammerandchisel.discord"},
				},
			},
		}),
	})
	if err := addURLSchemeToIPA(path, astralURLScheme); err != nil {
		t.Fatal(err)
	}
	if err := addURLSchemeToIPA(path, astralURLScheme); err != nil {
		t.Fatal(err)
	}
	entries := readIPAEntries(t, path)
	var info map[string]interface{}
	if _, err := plist.Unmarshal(entries[app+"Info.plist"], &info); err != nil {
		t.Fatal(err)
	}
	urlTypes, ok := info["CFBundleURLTypes"].([]interface{})
	if !ok {
		t.Fatalf("URL types have unexpected type %T", info["CFBundleURLTypes"])
	}
	counts := map[string]int{}
	for _, value := range urlTypes {
		typeInfo, ok := value.(map[string]interface{})
		if !ok {
			continue
		}
		schemes, _ := typeInfo["CFBundleURLSchemes"].([]interface{})
		for _, scheme := range schemes {
			if text, ok := scheme.(string); ok {
				counts[text]++
			}
		}
	}
	if counts["discord"] != 1 || counts["com.hammerandchisel.discord"] != 1 || counts[astralURLScheme] != 1 {
		t.Fatalf("URL schemes were not preserved and added once: %v", counts)
	}
}

func TestValidateIPAExtensions(t *testing.T) {
	source := []string{
		"Payload/Discord.app/PlugIns/BroadcastUpload.appex/Info.plist",
		"Payload/Discord.app/PlugIns/Share.appex/Info.plist",
	}
	complete := append(append([]string{}, source...), "Payload/Discord.app/PlugIns/OpenInAstral.appex/Info.plist")
	if err := validateIPAExtensions(source, complete, []string{"OpenInAstral.appex"}); err != nil {
		t.Fatalf("complete extension set rejected: %v", err)
	}
	if err := validateIPAExtensions(source, source[:1], nil); err == nil || !strings.Contains(err.Error(), "Share.appex") {
		t.Fatalf("missing share extension was not detected: %v", err)
	}
	if err := validateIPAExtensions(source, source, []string{"OpenInAstral.appex"}); err == nil || !strings.Contains(err.Error(), "OpenInAstral.appex") {
		t.Fatalf("missing astral extension was not detected: %v", err)
	}
}

func TestExtensionBundleIDUsesParentBundleIdentifier(t *testing.T) {
	got, err := extensionBundleID("com.discord", "app.astral.discord", "com.discord.Share")
	if err != nil || got != "app.astral.discord.Share" {
		t.Fatalf("extension bundle id mapping returned %q: %v", got, err)
	}
	if _, err := extensionBundleID("com.discord", "app.astral.discord", "com.other.Share"); err == nil {
		t.Fatal("extension outside the app identifier was accepted")
	}
}
