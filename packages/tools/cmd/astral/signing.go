package main

import (
	"crypto/ed25519"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/pem"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/pelletier/go-toml/v2"
)

const signatureVersion = "astral-plugin-v2"

type pluginManifest struct {
	ID          string   `toml:"id"`
	Main        string   `toml:"main"`
	Permissions []string `toml:"permissions"`
}

type pluginSignature struct {
	Signature string            `toml:"signature"`
	Files     map[string]string `toml:"files"`
}

func readManifest(folder string) (pluginManifest, error) {
	var manifest pluginManifest
	data, err := os.ReadFile(filepath.Join(folder, "manifest.toml"))
	if err != nil {
		return manifest, err
	}
	if err := toml.Unmarshal(data, &manifest); err != nil {
		return manifest, fmt.Errorf("%s/manifest.toml: %w", filepath.Base(folder), err)
	}
	if manifest.Main == "" {
		manifest.Main = "main.lua"
	}
	return manifest, nil
}

func signedPaths(folder string) ([]string, error) {
	manifest, err := readManifest(folder)
	if err != nil {
		return nil, err
	}
	paths := map[string]bool{"manifest.toml": true, manifest.Main: true}
	translations, err := filepath.Glob(filepath.Join(folder, "i18n", "*.toml"))
	if err != nil {
		return nil, err
	}
	for _, translation := range translations {
		paths["i18n/"+filepath.Base(translation)] = true
	}
	modules, err := pluginModules(folder)
	if err != nil {
		return nil, err
	}
	for _, module := range modules {
		paths[module] = true
		paths[bytecodePath(module)] = true
	}
	sorted := make([]string, 0, len(paths))
	for path := range paths {
		sorted = append(sorted, path)
	}
	sort.Strings(sorted)
	return sorted, nil
}

func pluginModules(folder string) ([]string, error) {
	var modules []string
	err := filepath.WalkDir(folder, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(folder, path)
		if err != nil {
			return err
		}
		if entry.IsDir() && relative == "tests" {
			return filepath.SkipDir
		}
		if !entry.IsDir() && filepath.Ext(path) == ".lua" {
			modules = append(modules, filepath.ToSlash(relative))
		}
		return nil
	})
	return modules, err
}

func digest(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:]), nil
}

func signatureMessage(files map[string]string) []byte {
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	var message strings.Builder
	message.WriteString(signatureVersion + "\n")
	for _, path := range paths {
		message.WriteString(path + " " + files[path] + "\n")
	}
	return []byte(message.String())
}

func publicKey() (ed25519.PublicKey, error) {
	text, err := os.ReadFile(repoPath("keys", "plugin-signing.pub"))
	if err != nil {
		return nil, err
	}
	key, err := base64.StdEncoding.DecodeString(strings.TrimSpace(string(text)))
	if err != nil || len(key) != ed25519.PublicKeySize {
		return nil, errors.New("keys/plugin-signing.pub is not a base64 Ed25519 public key")
	}
	return ed25519.PublicKey(key), nil
}

func privateKey(path string) (ed25519.PrivateKey, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("signing key not found: %w", err)
	}
	block, _ := pem.Decode(data)
	if block == nil {
		return nil, errors.New("signing key is not PEM")
	}
	parsed, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return nil, err
	}
	key, ok := parsed.(ed25519.PrivateKey)
	if !ok {
		return nil, errors.New("signing key is not Ed25519")
	}
	return key, nil
}

func pluginFiles(folder string) (map[string]string, error) {
	paths, err := signedPaths(folder)
	if err != nil {
		return nil, err
	}
	files := make(map[string]string, len(paths))
	for _, path := range paths {
		sum, err := digest(filepath.Join(folder, path))
		if err != nil {
			return nil, err
		}
		files[path] = sum
	}
	return files, nil
}

func writePluginSignature(folder string, key ed25519.PrivateKey) error {
	if err := compilePlugin(folder); err != nil {
		return err
	}
	files, err := pluginFiles(folder)
	if err != nil {
		return err
	}
	signature := base64.StdEncoding.EncodeToString(ed25519.Sign(key, signatureMessage(files)))
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	var document strings.Builder
	document.WriteString("signature = \"" + signature + "\"\n\n[files]\n")
	for _, path := range paths {
		document.WriteString("\"" + path + "\" = \"" + files[path] + "\"\n")
	}
	return os.WriteFile(filepath.Join(folder, "signature"), []byte(document.String()), 0o644)
}

func readPluginSignature(folder string) (pluginSignature, error) {
	var signature pluginSignature
	data, err := os.ReadFile(filepath.Join(folder, "signature"))
	if err != nil {
		return signature, err
	}
	return signature, toml.Unmarshal(data, &signature)
}

func verifyPlugin(folder string, key ed25519.PublicKey) error {
	name := filepath.Base(folder)
	signature, err := readPluginSignature(folder)
	if err != nil {
		return fmt.Errorf("%s: %w", name, err)
	}
	current, err := pluginFiles(folder)
	if err != nil {
		return fmt.Errorf("%s: %w", name, err)
	}
	if len(current) != len(signature.Files) {
		return fmt.Errorf("%s: signature lists different files from the folder (run astral sign)", name)
	}
	for path, sum := range current {
		if signature.Files[path] != sum {
			return fmt.Errorf("%s: %s changed since it was signed (run astral sign)", name, path)
		}
	}
	raw, err := base64.StdEncoding.DecodeString(signature.Signature)
	if err != nil || !ed25519.Verify(key, signatureMessage(signature.Files), raw) {
		return fmt.Errorf("%s: invalid signature (run astral sign)", name)
	}
	return nil
}

func verifyCatalog(key ed25519.PublicKey) error {
	index, err := os.ReadFile(repoPath("packages", "catalog", "index.toml"))
	if err != nil {
		return err
	}
	text, err := os.ReadFile(repoPath("packages", "catalog", "index.toml.sig"))
	if err != nil {
		return err
	}
	raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(string(text)))
	if err != nil || !ed25519.Verify(key, index, raw) {
		return errors.New("stale or invalid catalog signature (run astral sign)")
	}
	return nil
}

func defaultSigningKey() string {
	if path := os.Getenv("ASTRAL_SIGNING_KEY_FILE"); path != "" {
		return path
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, "Library", "Mobile Documents", "com~apple~CloudDocs", "astral-plugin-signing", "astral-plugin-signing.pem")
}

func runSign(args []string) error {
	flags := newFlags("sign")
	keyPath := flags.String("key", defaultSigningKey(), "Ed25519 private key (PEM)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	key, err := privateKey(*keyPath)
	if err != nil {
		return err
	}
	folders, err := pluginFolders()
	if err != nil {
		return err
	}
	for _, folder := range folders {
		if err := writePluginSignature(folder, key); err != nil {
			return fmt.Errorf("%s: %w", filepath.Base(folder), err)
		}
	}
	index, err := os.ReadFile(repoPath("packages", "catalog", "index.toml"))
	if err != nil {
		return err
	}
	signature := base64.StdEncoding.EncodeToString(ed25519.Sign(key, index))
	return os.WriteFile(repoPath("packages", "catalog", "index.toml.sig"), []byte(signature), 0o644)
}
