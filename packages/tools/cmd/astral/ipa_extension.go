package main

import (
	"archive/zip"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

func addOpenInAstralToIPA(ipa string, extension string) error {
	archive, err := zip.OpenReader(ipa)
	if err != nil {
		return err
	}
	app, err := appDirectory(archive)
	if err != nil {
		archive.Close()
		return err
	}
	prefix := app + "PlugIns/OpenInAstral.appex/"
	temporary, err := os.CreateTemp(filepath.Dir(ipa), ".astral-extension-*.ipa")
	if err != nil {
		archive.Close()
		return err
	}
	defer os.Remove(temporary.Name())
	writer := zip.NewWriter(temporary)
	for _, file := range archive.File {
		if strings.HasPrefix(file.Name, prefix) {
			continue
		}
		if err := writer.Copy(file); err != nil {
			temporary.Close()
			archive.Close()
			return err
		}
	}
	err = filepath.WalkDir(extension, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return errors.New("OpenInAstral.appex contains a non-regular file: " + path)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(extension, path)
		if err != nil {
			return err
		}
		name := prefix + filepath.ToSlash(relative)
		if err := writeIPAEntry(writer, name, info.Mode(), info.ModTime(), data); err != nil {
			return err
		}
		return nil
	})
	if err != nil {
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
	if err := os.Rename(temporary.Name(), ipa); err != nil {
		return fmt.Errorf("installing OpenInAstral.appex in the IPA failed: %w", err)
	}
	return nil
}
