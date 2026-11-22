package main

import (
	"archive/zip"
	"debug/macho"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"howett.net/plist"
)

const (
	cyanRepository = "https://github.com/asdfzxcvbn/pyzule-rw"
	cyanCommit     = "740d3716dcd98c20c000f12cdb88f1f0b2a533a4"
	pillowVersion  = "11.3.0"
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
	if err := copyFile(built, *destination); err != nil {
		return err
	}
	fmt.Println("created " + *destination)
	return nil
}
