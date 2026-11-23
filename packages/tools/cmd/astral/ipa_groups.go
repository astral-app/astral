package main

import (
	"archive/zip"
	"bytes"
	"debug/macho"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"howett.net/plist"
)

const (
	groupRemapFramework = "Frameworks/AstralGroups.dylib"
	groupRemapInstall   = "@rpath/AstralGroups.dylib"
)

var ownExtensions = map[string]bool{
	"OpenInAstral.appex": true,
}

type ipaExtension struct {
	name       string
	infoPath   string
	executable string
	bundleID   string
}

func buildGroupRemap(work string) (string, error) {
	source := repoPath("packages", "ios", "groups", "AstralGroups.m")
	output := filepath.Join(work, "AstralGroups.dylib")
	args := []string{
		"-sdk", "iphoneos",
		"clang",
		"-dynamiclib",
		"-arch", "arm64",
		"-miphoneos-version-min=18.0",
		"-fobjc-arc",
		"-framework", "Foundation",
		"-install_name", groupRemapInstall,
		"-o", output,
		source,
	}
	if err := run(work, "xcrun", args...); err != nil {
		return "", fmt.Errorf("building AstralGroups.dylib failed: %w", err)
	}
	return output, nil
}

func validateGroupRemapLibrary(data []byte) error {
	file, err := macho.NewFile(bytes.NewReader(data))
	if err != nil {
		return fmt.Errorf("AstralGroups.dylib is not Mach-O: %w", err)
	}
	if file.Magic != macho.Magic64 || file.ByteOrder != binary.LittleEndian || file.Cpu != macho.CpuArm64 || file.Type != macho.TypeDylib {
		return errors.New("AstralGroups.dylib must be an arm64 dylib")
	}
	if !machoLoadsDylib(file, groupRemapInstall) {
		return fmt.Errorf("AstralGroups.dylib has no install name %s", groupRemapInstall)
	}
	return nil
}

func readZipFile(file *zip.File) ([]byte, error) {
	reader, err := file.Open()
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	return io.ReadAll(reader)
}

func readIPAExtension(entries map[string]*zip.File, app string, infoPath string) (ipaExtension, error) {
	file := entries[infoPath]
	if file == nil {
		return ipaExtension{}, errors.New("extension has no Info.plist: " + infoPath)
	}
	data, err := readZipFile(file)
	if err != nil {
		return ipaExtension{}, err
	}
	var info map[string]interface{}
	if _, err := plist.Unmarshal(data, &info); err != nil {
		return ipaExtension{}, err
	}
	name := strings.TrimSuffix(filepath.Base(filepath.Dir(infoPath)), ".appex") + ".appex"
	executable, _ := info["CFBundleExecutable"].(string)
	bundleID, _ := info["CFBundleIdentifier"].(string)
	if executable == "" || filepath.Base(executable) != executable || executable == "." || executable == ".." {
		return ipaExtension{}, errors.New("extension has an invalid CFBundleExecutable: " + name)
	}
	return ipaExtension{
		name:       name,
		infoPath:   infoPath,
		executable: app + "PlugIns/" + name + "/" + executable,
		bundleID:   bundleID,
	}, nil
}

func ipaExtensions(archive *zip.ReadCloser, app string) ([]ipaExtension, map[string]*zip.File, error) {
	entries := make(map[string]*zip.File, len(archive.File))
	for _, file := range archive.File {
		entries[file.Name] = file
	}
	var paths []string
	for _, file := range archive.File {
		rest, ok := strings.CutPrefix(file.Name, app+"PlugIns/")
		parts := strings.Split(rest, "/")
		if ok && len(parts) == 2 && parts[1] == "Info.plist" && strings.HasSuffix(parts[0], ".appex") {
			paths = append(paths, file.Name)
		}
	}
	sort.Strings(paths)
	extensions := make([]ipaExtension, 0, len(paths))
	for _, path := range paths {
		extension, err := readIPAExtension(entries, app, path)
		if err != nil {
			return nil, nil, err
		}
		extensions = append(extensions, extension)
	}
	return extensions, entries, nil
}

func writeIPAEntry(writer *zip.Writer, name string, mode os.FileMode, modified time.Time, data []byte) error {
	header := &zip.FileHeader{Name: name, Method: zip.Deflate, Modified: modified}
	header.SetMode(mode)
	entry, err := writer.CreateHeader(header)
	if err != nil {
		return err
	}
	_, err = entry.Write(data)
	return err
}

func addGroupRemapToIPA(ipa string, library string) error {
	dylib, err := os.ReadFile(library)
	if err != nil {
		return err
	}
	if err := validateGroupRemapLibrary(dylib); err != nil {
		return err
	}
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return err
	}
	defer archive.Close()
	app, err := appDirectory(archive)
	if err != nil {
		return err
	}
	extensions, _, err := ipaExtensions(archive, app)
	if err != nil {
		return err
	}
	temporary, err := os.CreateTemp(filepath.Dir(ipa), ".astral-groups-*.ipa")
	if err != nil {
		return err
	}
	defer os.Remove(temporary.Name())
	writer := zip.NewWriter(temporary)
	for _, file := range archive.File {
		if file.Name == app+groupRemapFramework {
			continue
		}
		var patch *ipaExtension
		for index := range extensions {
			if file.Name == extensions[index].executable && !ownExtensions[extensions[index].name] {
				patch = &extensions[index]
				break
			}
		}
		if patch != nil {
			data, err := readZipFile(file)
			if err != nil {
				temporary.Close()
				return err
			}
			data, err = addWeakDylibBytes(data, groupRemapInstall)
			if err != nil {
				temporary.Close()
				return fmt.Errorf("wiring AstralGroups.dylib into %s failed: %w", patch.name, err)
			}
			if err := writeIPAEntry(writer, file.Name, file.Mode(), file.Modified, data); err != nil {
				temporary.Close()
				return err
			}
			continue
		}
		if err := writer.Copy(file); err != nil {
			temporary.Close()
			return err
		}
	}
	if err := writeIPAEntry(writer, app+groupRemapFramework, 0o755, time.Now(), dylib); err != nil {
		temporary.Close()
		return err
	}
	if err := writer.Close(); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := archive.Close(); err != nil {
		return err
	}
	return os.Rename(temporary.Name(), ipa)
}

func validateIPAGroupWiring(ipa string) error {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return err
	}
	defer archive.Close()
	app, err := appDirectory(archive)
	if err != nil {
		return err
	}
	extensions, entries, err := ipaExtensions(archive, app)
	if err != nil {
		return err
	}
	library := entries[app+groupRemapFramework]
	if library == nil {
		return fmt.Errorf("final IPA has no %s", groupRemapFramework)
	}
	libraryData, err := readZipFile(library)
	if err != nil {
		return err
	}
	if err := validateGroupRemapLibrary(libraryData); err != nil {
		return err
	}
	appInfo, err := readZipFile(entries[app+"Info.plist"])
	if err != nil {
		return err
	}
	var info map[string]interface{}
	if _, err := plist.Unmarshal(appInfo, &info); err != nil {
		return err
	}
	appID, _ := info["CFBundleIdentifier"].(string)
	if appID == "" {
		return errors.New("the app has no CFBundleIdentifier")
	}
	appExecutable, _ := info["CFBundleExecutable"].(string)
	appGroups := []string(nil)
	if appExecutable != "" {
		if file := entries[app+appExecutable]; file != nil {
			data, err := readZipFile(file)
			if err != nil {
				return err
			}
			appGroups = machoAppGroups(data)
		}
	}
	foundShare := false
	foundAstral := false
	for _, extension := range extensions {
		if !strings.HasPrefix(extension.bundleID, appID+".") {
			return fmt.Errorf("%s has bundle identifier %q outside %s", extension.name, extension.bundleID, appID)
		}
		if ownExtensions[extension.name] {
			foundAstral = true
			manifestFile := entries[app+"PlugIns/"+extension.name+"/manifest.json"]
			if manifestFile == nil {
				return errors.New(extension.name + " has no Safari Web Extension manifest")
			}
			manifestData, err := readZipFile(manifestFile)
			if err != nil {
				return err
			}
			var manifest struct {
				Version int `json:"manifest_version"`
			}
			if err := json.Unmarshal(manifestData, &manifest); err != nil || manifest.Version != 3 {
				return fmt.Errorf("%s has an invalid Safari Web Extension manifest", extension.name)
			}
			continue
		}
		if extension.name == "Share.appex" {
			foundShare = true
		}
		file := entries[extension.executable]
		if file == nil {
			return fmt.Errorf("%s has no executable at %s", extension.name, extension.executable)
		}
		data, err := readZipFile(file)
		if err != nil {
			return err
		}
		machoFile, err := macho.NewFile(bytes.NewReader(data))
		if err != nil || !machoLoadsDylib(machoFile, groupRemapInstall) {
			return fmt.Errorf("%s does not load AstralGroups.dylib", extension.name)
		}
		for _, group := range machoAppGroups(data) {
			if !containsGroup(appGroups, group) {
				return fmt.Errorf("%s uses app group %s, which the app does not", extension.name, group)
			}
		}
	}
	if !foundShare {
		return errors.New("final IPA has no Discord Share.appex")
	}
	if !foundAstral {
		return errors.New("final IPA has no OpenInAstral.appex")
	}
	return nil
}

func validateSignedAppGroupWiring(app string) error {
	readInfo := func(path string) (map[string]interface{}, error) {
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, err
		}
		var info map[string]interface{}
		if _, err := plist.Unmarshal(data, &info); err != nil {
			return nil, err
		}
		return info, nil
	}
	appInfo, err := readInfo(filepath.Join(app, "Info.plist"))
	if err != nil {
		return err
	}
	appID, _ := appInfo["CFBundleIdentifier"].(string)
	appExecutable, _ := appInfo["CFBundleExecutable"].(string)
	if appID == "" || appExecutable == "" || filepath.Base(appExecutable) != appExecutable {
		return errors.New("signed app has invalid bundle identity")
	}
	appBinary, err := os.ReadFile(filepath.Join(app, appExecutable))
	if err != nil {
		return err
	}
	appGroups := machoAppGroups(appBinary)
	library, err := os.ReadFile(filepath.Join(app, groupRemapFramework))
	if err != nil {
		return fmt.Errorf("signed app has no %s: %w", groupRemapFramework, err)
	}
	if err := validateGroupRemapLibrary(library); err != nil {
		return err
	}
	bundles, err := extensionBundles(app)
	if err != nil {
		return err
	}
	foundShare := false
	foundAstral := false
	for _, bundle := range bundles {
		name := filepath.Base(bundle)
		info, err := readInfo(filepath.Join(bundle, "Info.plist"))
		if err != nil {
			return fmt.Errorf("%s has no readable Info.plist: %w", name, err)
		}
		identifier, _ := info["CFBundleIdentifier"].(string)
		if !strings.HasPrefix(identifier, appID+".") {
			return fmt.Errorf("%s has bundle identifier %q outside %s", name, identifier, appID)
		}
		if name == "Share.appex" {
			foundShare = true
		}
		if ownExtensions[name] {
			foundAstral = true
			continue
		}
		executable, _ := info["CFBundleExecutable"].(string)
		if executable == "" || filepath.Base(executable) != executable {
			return fmt.Errorf("%s has an invalid executable name", name)
		}
		data, err := os.ReadFile(filepath.Join(bundle, executable))
		if err != nil {
			return fmt.Errorf("%s has no readable executable: %w", name, err)
		}
		binary, err := macho.NewFile(bytes.NewReader(data))
		if err != nil || !machoLoadsDylib(binary, groupRemapInstall) {
			return fmt.Errorf("%s does not load AstralGroups.dylib", name)
		}
		for _, group := range machoAppGroups(data) {
			if !containsGroup(appGroups, group) {
				return fmt.Errorf("%s uses app group %s, which the app does not", name, group)
			}
		}
	}
	if !foundShare {
		return errors.New("signed app has no Discord Share.appex")
	}
	if !foundAstral {
		return errors.New("signed app has no OpenInAstral.appex")
	}
	return nil
}

func containsGroup(groups []string, target string) bool {
	for _, group := range groups {
		if group == target {
			return true
		}
	}
	return false
}

func validateIPAExtensions(sourceEntries []string, finalEntries []string, required []string) error {
	source := extensionNames(sourceEntries)
	final := extensionNames(finalEntries)
	for extension := range source {
		if !final[extension] {
			return errors.New("IPA build dropped the Discord extension " + extension)
		}
	}
	for _, extension := range required {
		if !final[extension] {
			return errors.New("IPA build omitted astral's extension " + extension)
		}
	}
	return nil
}

func extensionNames(entries []string) map[string]bool {
	result := map[string]bool{}
	for _, entry := range entries {
		parts := strings.Split(entry, "/")
		if len(parts) == 5 && parts[0] == "Payload" && strings.HasSuffix(parts[1], ".app") &&
			parts[2] == "PlugIns" && strings.HasSuffix(parts[3], ".appex") && parts[4] == "Info.plist" {
			result[parts[3]] = true
		}
	}
	return result
}
