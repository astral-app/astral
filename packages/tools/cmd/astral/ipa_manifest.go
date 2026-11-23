package main

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

const extensionManifestFilename = "AstralExtensions.json"

type extensionManifest struct {
	Extensions []string `json:"extensions"`
}

func extensionManifestFor(sourceEntries []string, required []string) ([]byte, error) {
	expected := extensionNames(sourceEntries)
	for _, extension := range required {
		if !validExtensionName(extension) {
			return nil, errors.New("invalid required extension name: " + extension)
		}
		expected[extension] = true
	}
	if len(expected) == 0 {
		return nil, errors.New("the source IPA contains no app extensions")
	}
	names := make([]string, 0, len(expected))
	for extension := range expected {
		names = append(names, extension)
	}
	sort.Strings(names)
	return json.Marshal(extensionManifest{Extensions: names})
}

func validExtensionName(name string) bool {
	return strings.HasSuffix(name, ".appex") && filepath.Base(name) == name && name != ".appex"
}

func addExtensionManifestToIPA(ipa string, sourceEntries []string, required []string) error {
	manifest, err := extensionManifestFor(sourceEntries, required)
	if err != nil {
		return err
	}
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return err
	}
	app, err := appDirectory(archive)
	if err != nil {
		archive.Close()
		return err
	}
	temporary, err := os.CreateTemp(filepath.Dir(ipa), ".astral-extension-manifest-*.ipa")
	if err != nil {
		archive.Close()
		return err
	}
	defer os.Remove(temporary.Name())
	writer := zip.NewWriter(temporary)
	manifestPath := app + extensionManifestFilename
	for _, file := range archive.File {
		if file.Name == manifestPath {
			continue
		}
		if err := writer.Copy(file); err != nil {
			temporary.Close()
			archive.Close()
			return err
		}
	}
	if err := writeIPAEntry(writer, manifestPath, 0o644, time.Now(), manifest); err != nil {
		temporary.Close()
		archive.Close()
		return err
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

func validateSignedAppExtensions(app string) error {
	data, err := os.ReadFile(filepath.Join(app, extensionManifestFilename))
	if err != nil {
		return fmt.Errorf("signed app extension manifest is unavailable: %w", err)
	}
	var manifest extensionManifest
	if err := json.Unmarshal(data, &manifest); err != nil || len(manifest.Extensions) == 0 {
		return errors.New("signed app extension manifest is invalid")
	}
	expected := map[string]bool{}
	for _, extension := range manifest.Extensions {
		if !validExtensionName(extension) || expected[extension] {
			return errors.New("signed app extension manifest contains an invalid entry")
		}
		expected[extension] = true
	}
	bundles, err := extensionBundles(app)
	if err != nil {
		return err
	}
	for _, bundle := range bundles {
		delete(expected, filepath.Base(bundle))
	}
	if len(expected) == 0 {
		return nil
	}
	missing := make([]string, 0, len(expected))
	for extension := range expected {
		missing = append(missing, extension)
	}
	sort.Strings(missing)
	return fmt.Errorf("signed app is missing required extensions: %s", strings.Join(missing, ", "))
}
