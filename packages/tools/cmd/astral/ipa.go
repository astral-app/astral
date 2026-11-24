package main

import (
	"archive/zip"
	"debug/macho"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"howett.net/plist"
)

const (
	cyanRepository  = "https://github.com/asdfzxcvbn/pyzule-rw"
	cyanCommit      = "740d3716dcd98c20c000f12cdb88f1f0b2a533a4"
	pillowVersion   = "11.3.0"
	astralURLScheme = "astral"
)

func cyanBinary() (string, error) {
	cache := repoPath(".tools-cache", "cyan-"+cyanCommit)
	binary := filepath.Join(cache, "venv", "bin", "cyan")
	if fileExists(binary) {
		return binary, nil
	}
	if err := os.MkdirAll(cache, 0o755); err != nil {
		return "", err
	}
	venv := filepath.Join(cache, "venv")
	if err := run(cache, "python3", "-m", "venv", venv); err != nil {
		return "", err
	}
	archive := cyanRepository + "/archive/" + cyanCommit + ".zip"
	if err := run(cache, filepath.Join(venv, "bin", "python3"), "-m", "pip", "install", "--disable-pip-version-check", "--quiet", archive, "Pillow=="+pillowVersion); err != nil {
		return "", err
	}
	return binary, nil
}

func readZipEntry(archive *zip.ReadCloser, name string) ([]byte, error) {
	for _, file := range archive.File {
		if file.Name != name {
			continue
		}
		reader, err := file.Open()
		if err != nil {
			return nil, err
		}
		defer reader.Close()
		return io.ReadAll(reader)
	}
	return nil, errors.New(name + " is not in the IPA")
}

func appDirectory(archive *zip.ReadCloser) (string, error) {
	for _, file := range archive.File {
		parts := strings.Split(file.Name, "/")
		if len(parts) >= 3 && parts[0] == "Payload" && strings.HasSuffix(parts[1], ".app") && parts[2] == "Info.plist" {
			return "Payload/" + parts[1] + "/", nil
		}
	}
	return "", errors.New("the IPA has no Payload/*.app/Info.plist")
}

func loadsAstral(ipa string, work string) (bool, error) {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return false, err
	}
	defer archive.Close()
	app, err := appDirectory(archive)
	if err != nil {
		return false, err
	}
	info, err := readZipEntry(archive, app+"Info.plist")
	if err != nil {
		return false, err
	}
	var bundle struct {
		Executable string `plist:"CFBundleExecutable"`
	}
	if _, err := plist.Unmarshal(info, &bundle); err != nil {
		return false, err
	}
	executable, err := readZipEntry(archive, app+bundle.Executable)
	if err != nil {
		return false, err
	}
	path := filepath.Join(work, "executable")
	if err := os.WriteFile(path, executable, 0o644); err != nil {
		return false, err
	}
	file, err := macho.Open(path)
	if err != nil {
		return false, err
	}
	defer file.Close()
	for _, library := range loadedDylibs(file) {
		if strings.HasSuffix(library, "/astral.dylib") {
			return true, nil
		}
	}
	return false, nil
}

func loadedDylibs(file *macho.File) []string {
	const loadWeakDylib = 0x80000018
	var libraries []string
	for _, load := range file.Loads {
		raw := load.Raw()
		if len(raw) < 12 {
			continue
		}
		command := file.ByteOrder.Uint32(raw[0:4])
		if command != uint32(macho.LoadCmdDylib) && command != loadWeakDylib {
			continue
		}
		offset := file.ByteOrder.Uint32(raw[8:12])
		if int(offset) >= len(raw) {
			continue
		}
		name := raw[offset:]
		if end := strings.IndexByte(string(name), 0); end >= 0 {
			name = name[:end]
		}
		libraries = append(libraries, string(name))
	}
	return libraries
}

func ipaBundleIdentifier(ipa string) (string, error) {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return "", err
	}
	defer archive.Close()
	app, err := appDirectory(archive)
	if err != nil {
		return "", err
	}
	data, err := readZipEntry(archive, app+"Info.plist")
	if err != nil {
		return "", err
	}
	var info map[string]interface{}
	if _, err := plist.Unmarshal(data, &info); err != nil {
		return "", err
	}
	identifier, _ := info["CFBundleIdentifier"].(string)
	if identifier == "" {
		return "", errors.New("the app has no CFBundleIdentifier")
	}
	return identifier, nil
}

func ensureURLScheme(info map[string]interface{}, scheme string) bool {
	urlTypes, _ := info["CFBundleURLTypes"].([]interface{})
	for _, value := range urlTypes {
		typeInfo, ok := value.(map[string]interface{})
		if !ok {
			continue
		}
		schemes, _ := typeInfo["CFBundleURLSchemes"].([]interface{})
		for _, existing := range schemes {
			if existing == scheme {
				return false
			}
		}
	}
	info["CFBundleURLTypes"] = append(urlTypes, map[string]interface{}{
		"CFBundleURLName":    "app.astral.client",
		"CFBundleURLSchemes": []interface{}{scheme},
	})
	return true
}

func addURLSchemeToIPA(ipa string, scheme string) error {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return err
	}
	app, err := appDirectory(archive)
	if err != nil {
		archive.Close()
		return err
	}
	infoPath := app + "Info.plist"
	data, err := readZipEntry(archive, infoPath)
	if err != nil {
		archive.Close()
		return err
	}
	var info map[string]interface{}
	format, err := plist.Unmarshal(data, &info)
	if err != nil {
		archive.Close()
		return err
	}
	if !ensureURLScheme(info, scheme) {
		return archive.Close()
	}
	encoded, err := plist.Marshal(info, format)
	if err != nil {
		archive.Close()
		return err
	}
	temporary, err := os.CreateTemp(filepath.Dir(ipa), ".astral-url-scheme-*.ipa")
	if err != nil {
		archive.Close()
		return err
	}
	defer os.Remove(temporary.Name())
	writer := zip.NewWriter(temporary)
	for _, file := range archive.File {
		if file.Name == infoPath {
			if err := writeIPAEntry(writer, file.Name, file.Mode(), file.Modified, encoded); err != nil {
				temporary.Close()
				archive.Close()
				return err
			}
			continue
		}
		if err := writer.Copy(file); err != nil {
			temporary.Close()
			archive.Close()
			return err
		}
	}
	if err := writer.Close(); err != nil {
		temporary.Close()
		archive.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		archive.Close()
		return err
	}
	if err := archive.Close(); err != nil {
		return err
	}
	return os.Rename(temporary.Name(), ipa)
}

func buildOpenInAstralExtension(work string, appBundleID string) (string, error) {
	if runtime.GOOS != "darwin" {
		return "", errors.New("Safari extensions require macOS")
	}
	project := repoPath("packages", "ios", "extensions", "OpenInAstral", "OpenInAstral.xcodeproj")
	output := filepath.Join(work, "OpenInAstral-build")
	args := []string{
		"-project", project,
		"-quiet",
		"-target", "OpenInAstral",
		"build",
		"-configuration", "Release",
		"-sdk", "iphoneos",
		"CONFIGURATION_BUILD_DIR=" + output,
		"PRODUCT_NAME=OpenInAstral",
		"PRODUCT_BUNDLE_IDENTIFIER=" + appBundleID + ".OpenInAstral",
		"CODE_SIGNING_REQUIRED=NO",
		"CODE_SIGNING_ALLOWED=NO",
		"ONLY_ACTIVE_ARCH=NO",
	}
	if err := run(work, "xcodebuild", args...); err != nil {
		return "", fmt.Errorf("building OpenInAstral.appex failed: %w", err)
	}
	path := filepath.Join(output, "OpenInAstral.appex")
	if !fileExists(path) {
		return "", errors.New("xcodebuild did not create OpenInAstral.appex")
	}
	return path, nil
}

func runBuildIPA(args []string) error {
	flags := newFlags("build-ipa")
	ipa := flags.String("ipa", "", "decrypted Discord IPA")
	deb := flags.String("deb", "", "astral .deb (default: the newest built one)")
	destination := flags.String("output", repoPath("astral.ipa"), "output IPA")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *ipa == "" {
		return errors.New("usage: astral build-ipa --ipa <decrypted Discord.ipa> [--deb <astral.deb>] [--output <astral.ipa>]")
	}
	if !fileExists(*ipa) {
		return errors.New("IPA not found: " + *ipa)
	}
	if *deb == "" {
		newest, err := newestDeb()
		if err != nil {
			return err
		}
		*deb = newest
	}
	for _, path := range []*string{ipa, deb, destination} {
		absolute, err := filepath.Abs(*path)
		if err != nil {
			return err
		}
		*path = absolute
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	injected, err := loadsAstral(*ipa, work)
	if err != nil {
		return err
	}
	if injected {
		return errors.New("the IPA already loads astral.dylib; use a clean Discord IPA")
	}
	sourceArchive, err := zip.OpenReader(*ipa)
	if err != nil {
		return err
	}
	sourceEntries := make([]string, 0, len(sourceArchive.File))
	for _, file := range sourceArchive.File {
		sourceEntries = append(sourceEntries, file.Name)
	}
	if err := sourceArchive.Close(); err != nil {
		return err
	}
	cyan, err := cyanBinary()
	if err != nil {
		return err
	}
	bundle := filepath.Join(work, "astral.bundle")
	if err := stageResources(bundle); err != nil {
		return err
	}
	built := filepath.Join(work, "astral.ipa")
	if err := run(work, cyan, "-duwsgq", "-i", *ipa, "-o", built, "-f", *deb, bundle); err != nil {
		return err
	}
	if err := addURLSchemeToIPA(built, astralURLScheme); err != nil {
		return fmt.Errorf("registering astral's URL scheme failed: %w", err)
	}
	appBundleID, err := ipaBundleIdentifier(*ipa)
	if err != nil {
		return err
	}
	extension, err := buildOpenInAstralExtension(work, appBundleID)
	if err != nil {
		return err
	}
	if err := addOpenInAstralToIPA(built, extension); err != nil {
		return err
	}
	if err := addExtensionManifestToIPA(built, sourceEntries, []string{"OpenInAstral.appex", "Share.appex"}); err != nil {
		return fmt.Errorf("recording required app extensions failed: %w", err)
	}
	groups, err := buildGroupRemap(work)
	if err != nil {
		return err
	}
	if err := addGroupRemapToIPA(built, groups); err != nil {
		return err
	}
	if err := validateIPAGroupWiring(built); err != nil {
		return err
	}
	finalArchive, err := zip.OpenReader(built)
	if err != nil {
		return err
	}
	finalEntries := make([]string, 0, len(finalArchive.File))
	for _, file := range finalArchive.File {
		finalEntries = append(finalEntries, file.Name)
	}
	if err := finalArchive.Close(); err != nil {
		return err
	}
	if err := validateIPAExtensions(sourceEntries, finalEntries, []string{"OpenInAstral.appex", "Share.appex"}); err != nil {
		return err
	}
	if err := copyFile(built, *destination); err != nil {
		return err
	}
	fmt.Println("created " + *destination)
	return nil
}
