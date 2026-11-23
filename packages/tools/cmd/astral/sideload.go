package main

import (
	"archive/zip"
	"crypto/sha1"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"

	"howett.net/plist"
)

type provisioningProfile struct {
	path         string
	Name         string                 `plist:"Name"`
	Expiration   time.Time              `plist:"ExpirationDate"`
	Devices      []string               `plist:"ProvisionedDevices"`
	Certificates [][]byte               `plist:"DeveloperCertificates"`
	Entitlements map[string]interface{} `plist:"Entitlements"`
	Team         []string               `plist:"TeamIdentifier"`
}

func (profile provisioningProfile) applicationIdentifier() string {
	value, _ := profile.Entitlements["application-identifier"].(string)
	return value
}

func (profile provisioningProfile) firstApplicationGroup() string {
	groups := profile.applicationGroups()
	if len(groups) > 0 {
		return groups[0]
	}
	return ""
}

func (profile provisioningProfile) applicationGroups() []string {
	groups, _ := profile.Entitlements["com.apple.security.application-groups"].([]interface{})
	result := make([]string, 0, len(groups))
	for _, value := range groups {
		if group, ok := value.(string); ok && group != "" {
			result = append(result, group)
		}
	}
	return result
}

func (profile provisioningProfile) covers(bundleID string, device string) bool {
	if len(profile.Team) == 0 || time.Now().After(profile.Expiration) {
		return false
	}
	pattern := strings.TrimPrefix(profile.applicationIdentifier(), profile.Team[0]+".")
	matches := pattern == bundleID || (strings.HasSuffix(pattern, "*") && strings.HasPrefix(bundleID, strings.TrimSuffix(pattern, "*")))
	if !matches {
		return false
	}
	for _, provisioned := range profile.Devices {
		if strings.EqualFold(provisioned, device) {
			return true
		}
	}
	return false
}

func readProfile(path string) (provisioningProfile, error) {
	var profile provisioningProfile
	decoded, err := output(repoPath(), "security", "cms", "-D", "-i", path)
	if err != nil {
		return profile, err
	}
	if _, err := plist.Unmarshal([]byte(decoded), &profile); err != nil {
		return profile, err
	}
	profile.path = path
	return profile, nil
}

func findProfile(bundleID string, device string) (provisioningProfile, error) {
	home, _ := os.UserHomeDir()
	var candidates []string
	for _, folder := range []string{"Library/Developer/Xcode/UserData/Provisioning Profiles", "Library/MobileDevice/Provisioning Profiles"} {
		found, _ := filepath.Glob(filepath.Join(home, folder, "*.mobileprovision"))
		candidates = append(candidates, found...)
	}
	var matching []provisioningProfile
	for _, candidate := range candidates {
		profile, err := readProfile(candidate)
		if err == nil && profile.covers(bundleID, device) {
			matching = append(matching, profile)
		}
	}
	if len(matching) == 0 {
		return provisioningProfile{}, fmt.Errorf("no provisioning profile covers %s on %s; pass --profile", bundleID, device)
	}
	sort.Slice(matching, func(left, right int) bool {
		return !strings.Contains(matching[left].applicationIdentifier(), "*") && strings.Contains(matching[right].applicationIdentifier(), "*")
	})
	return matching[0], nil
}

func signingIdentity(profile provisioningProfile) (string, error) {
	listing, err := output(repoPath(), "security", "find-identity", "-v", "-p", "codesigning")
	if err != nil {
		return "", err
	}
	available := map[string]bool{}
	pattern := regexp.MustCompile(`\) ([0-9A-F]{40}) "`)
	for _, line := range strings.Split(listing, "\n") {
		if match := pattern.FindStringSubmatch(line); match != nil && !strings.Contains(line, "CSSMERR") {
			available[match[1]] = true
		}
	}
	for _, certificate := range profile.Certificates {
		sum := sha1.Sum(certificate)
		hash := strings.ToUpper(hex.EncodeToString(sum[:]))
		if available[hash] {
			return hash, nil
		}
	}
	return "", errors.New("no signing identity in the keychain matches " + profile.Name)
}

func unzip(archive string, destination string) error {
	reader, err := zip.OpenReader(archive)
	if err != nil {
		return err
	}
	defer reader.Close()
	for _, file := range reader.File {
		target := filepath.Join(destination, file.Name)
		if !strings.HasPrefix(target, filepath.Clean(destination)+string(os.PathSeparator)) {
			return errors.New("unsafe path in IPA: " + file.Name)
		}
		if file.FileInfo().IsDir() {
			if err := os.MkdirAll(target, 0o755); err != nil {
				return err
			}
			continue
		}
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		if file.Mode()&fs.ModeSymlink != 0 {
			source, err := file.Open()
			if err != nil {
				return err
			}
			link, err := io.ReadAll(source)
			source.Close()
			if err != nil {
				return err
			}
			if err := os.Symlink(string(link), target); err != nil {
				return err
			}
			continue
		}
		source, err := file.Open()
		if err != nil {
			return err
		}
		created, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, file.Mode()|0o600)
		if err != nil {
			source.Close()
			return err
		}
		_, copyErr := io.Copy(created, source)
		source.Close()
		created.Close()
		if copyErr != nil {
			return copyErr
		}
	}
	return nil
}

func entitlementsFor(profile provisioningProfile, bundleID string, work string) (string, error) {
	entitlements := map[string]interface{}{}
	for key, value := range profile.Entitlements {
		entitlements[key] = value
	}
	team := profile.Team[0]
	entitlements["application-identifier"] = team + "." + bundleID
	if groups, ok := entitlements["keychain-access-groups"].([]interface{}); ok {
		for index, group := range groups {
			if text, isText := group.(string); isText && strings.HasSuffix(text, "*") {
				groups[index] = team + "." + bundleID
			}
		}
	}
	encoded, err := plist.MarshalIndent(entitlements, plist.XMLFormat, "\t")
	if err != nil {
		return "", err
	}
	path := filepath.Join(work, "entitlements.plist")
	return path, os.WriteFile(path, encoded, 0o644)
}

func signBundle(app string, identity string, entitlements string) error {
	var nested []string
	filepath.WalkDir(filepath.Join(app, "Frameworks"), func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if entry.IsDir() && strings.HasSuffix(path, ".framework") {
			nested = append(nested, path)
			return filepath.SkipDir
		}
		if !entry.IsDir() && strings.HasSuffix(path, ".dylib") {
			nested = append(nested, path)
		}
		return nil
	})
	for _, item := range nested {
		if err := run(app, "codesign", "--force", "--sign", identity, "--timestamp=none", item); err != nil {
			return err
		}
	}
	return run(app, "codesign", "--force", "--sign", identity, "--timestamp=none", "--entitlements", entitlements, app)
}

func setBundleID(app string, bundleID string) error {
	info := filepath.Join(app, "Info.plist")
	data, err := os.ReadFile(info)
	if err != nil {
		return err
	}
	var values map[string]interface{}
	format, err := plist.Unmarshal(data, &values)
	if err != nil {
		return err
	}
	values["CFBundleIdentifier"] = bundleID
	ensureURLScheme(values, astralURLScheme)
	delete(values, "UISupportedDevices")
	encoded, err := plist.Marshal(values, format)
	if err != nil {
		return err
	}
	return os.WriteFile(info, encoded, 0o644)
}

func bundleIdentifier(bundle string) (string, error) {
	data, err := os.ReadFile(filepath.Join(bundle, "Info.plist"))
	if err != nil {
		return "", err
	}
	var values map[string]interface{}
	if _, err := plist.Unmarshal(data, &values); err != nil {
		return "", err
	}
	bundleID, _ := values["CFBundleIdentifier"].(string)
	if bundleID == "" {
		return "", errors.New("bundle has no CFBundleIdentifier: " + bundle)
	}
	return bundleID, nil
}

func setExtensionBundleID(bundle string, bundleID string) error {
	info := filepath.Join(bundle, "Info.plist")
	data, err := os.ReadFile(info)
	if err != nil {
		return err
	}
	var values map[string]interface{}
	format, err := plist.Unmarshal(data, &values)
	if err != nil {
		return err
	}
	values["CFBundleIdentifier"] = bundleID
	encoded, err := plist.Marshal(values, format)
	if err != nil {
		return err
	}
	return os.WriteFile(info, encoded, 0o644)
}

func extensionBundles(app string) ([]string, error) {
	plugins := filepath.Join(app, "PlugIns")
	entries, err := os.ReadDir(plugins)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var bundles []string
	for _, entry := range entries {
		if entry.IsDir() && strings.HasSuffix(entry.Name(), ".appex") {
			bundles = append(bundles, filepath.Join(plugins, entry.Name()))
		}
	}
	sort.Strings(bundles)
	return bundles, nil
}

func extensionBundleID(originalAppID string, targetAppID string, extensionID string) (string, error) {
	suffix, ok := strings.CutPrefix(extensionID, originalAppID)
	if !ok || !strings.HasPrefix(suffix, ".") {
		return "", fmt.Errorf("extension bundle identifier %q is not nested under %q", extensionID, originalAppID)
	}
	return targetAppID + suffix, nil
}

func profileForExtension(profile provisioningProfile, bundleID string, device string) (provisioningProfile, error) {
	if profile.covers(bundleID, device) {
		return profile, nil
	}
	return findProfile(bundleID, device)
}

func signExtensions(app string, originalAppID string, bundleID string, profile provisioningProfile, explicitProfile string, device string, work string) error {
	bundles, err := extensionBundles(app)
	if err != nil {
		return err
	}
	appGroup := profile.firstApplicationGroup()
	appGroups := profile.applicationGroups()
	requiresSharedGroup := false
	for _, bundle := range bundles {
		if !ownExtensions[filepath.Base(bundle)] {
			requiresSharedGroup = true
			break
		}
	}
	if requiresSharedGroup && appGroup == "" {
		return errors.New("the app provisioning profile has no application group entitlement for Discord's extensions")
	}
	for _, bundle := range bundles {
		extensionID, err := bundleIdentifier(bundle)
		if err != nil {
			return err
		}
		targetID, err := extensionBundleID(originalAppID, bundleID, extensionID)
		if err != nil {
			return err
		}
		extensionProfile, err := profileForExtension(profile, targetID, device)
		if err != nil {
			return err
		}
		if len(profile.Team) == 0 || len(extensionProfile.Team) == 0 || profile.Team[0] != extensionProfile.Team[0] {
			return fmt.Errorf("provisioning profile for extension %s must use the app's team", targetID)
		}
		if !ownExtensions[filepath.Base(bundle)] && extensionProfile.firstApplicationGroup() != appGroup {
			return fmt.Errorf("provisioning profile for extension %s must use the app's application group %q", targetID, appGroup)
		}
		if !ownExtensions[filepath.Base(bundle)] {
			for _, group := range extensionProfile.applicationGroups() {
				if !containsGroup(appGroups, group) {
					return fmt.Errorf("provisioning profile for extension %s adds app group %q that the app does not have", targetID, group)
				}
			}
		}
		extensionIdentity, err := signingIdentity(extensionProfile)
		if err != nil {
			return err
		}
		if err := setExtensionBundleID(bundle, targetID); err != nil {
			return err
		}
		if err := copyFile(extensionProfile.path, filepath.Join(bundle, "embedded.mobileprovision")); err != nil {
			return err
		}
		entitlementDirectory := filepath.Join(work, strings.TrimSuffix(filepath.Base(bundle), ".appex"))
		if err := os.MkdirAll(entitlementDirectory, 0o755); err != nil {
			return err
		}
		entitlements, err := entitlementsFor(extensionProfile, targetID, entitlementDirectory)
		if err != nil {
			return err
		}
		if err := signBundle(bundle, extensionIdentity, entitlements); err != nil {
			return err
		}
	}
	return nil
}

func runSideload(args []string) error {
	flags := newFlags("sideload")
	device := deviceFlag(flags)
	ipa := flags.String("ipa", "", "decrypted Discord IPA (astral is injected) or an IPA astral build-ipa made")
	bundleID := flags.String("bundle-id", "", "bundle identifier to install under")
	profilePath := flags.String("profile", "", "provisioning profile (default: one in Xcode's folder that covers the bundle id and device)")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *ipa == "" || *bundleID == "" {
		return errors.New("usage: astral sideload --ipa <IPA> --bundle-id <id> [--device <udid>] [--profile <mobileprovision>]")
	}
	if err := requireDevice(*device); err != nil {
		return err
	}
	var profile provisioningProfile
	var err error
	if *profilePath != "" {
		profile, err = readProfile(*profilePath)
	} else {
		profile, err = findProfile(*bundleID, *device)
	}
	if err != nil {
		return err
	}
	if !profile.covers(*bundleID, *device) {
		return fmt.Errorf("provisioning profile %s does not cover %s on %s", profile.Name, *bundleID, *device)
	}
	identity, err := signingIdentity(profile)
	if err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	source := *ipa
	injected, err := loadsAstral(source, work)
	if err != nil {
		return err
	}
	sourceArchive, err := zip.OpenReader(source)
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
	if !injected {
		built := filepath.Join(work, "astral.ipa")
		if err := runBuildIPA([]string{"--ipa", source, "--output", built}); err != nil {
			return err
		}
		source = built
	}
	groups, err := buildGroupRemap(work)
	if err != nil {
		return err
	}
	wired := filepath.Join(work, "wired.ipa")
	if err := copyFile(source, wired); err != nil {
		return err
	}
	if err := addExtensionManifestToIPA(wired, sourceEntries, []string{"OpenInAstral.appex", "Share.appex"}); err != nil {
		return fmt.Errorf("recording signed app extension requirements failed: %w", err)
	}
	if injected {
		appBundleID, err := ipaBundleIdentifier(wired)
		if err != nil {
			return err
		}
		extension, err := buildOpenInAstralExtension(work, appBundleID)
		if err != nil {
			return err
		}
		if err := addOpenInAstralToIPA(wired, extension); err != nil {
			return err
		}
	}
	if err := addGroupRemapToIPA(wired, groups); err != nil {
		return err
	}
	if err := validateIPAGroupWiring(wired); err != nil {
		return err
	}
	extracted := filepath.Join(work, "extracted")
	if err := unzip(wired, extracted); err != nil {
		return err
	}
	apps, _ := filepath.Glob(filepath.Join(extracted, "Payload", "*.app"))
	if len(apps) != 1 {
		return errors.New("the IPA has no single Payload/*.app")
	}
	app := apps[0]
	for _, extension := range []string{"Watch", "Extensions"} {
		os.RemoveAll(filepath.Join(app, extension))
	}
	originalBundleID, err := bundleIdentifier(app)
	if err != nil {
		return err
	}
	if err := setBundleID(app, *bundleID); err != nil {
		return err
	}
	if err := signExtensions(app, originalBundleID, *bundleID, profile, *profilePath, *device, work); err != nil {
		return err
	}
	if err := copyFile(profile.path, filepath.Join(app, "embedded.mobileprovision")); err != nil {
		return err
	}
	entitlements, err := entitlementsFor(profile, *bundleID, work)
	if err != nil {
		return err
	}
	if err := signBundle(app, identity, entitlements); err != nil {
		return err
	}
	if err := validateSignedAppExtensions(app); err != nil {
		return err
	}
	if err := validateSignedAppGroupWiring(app); err != nil {
		return err
	}
	if err := run(work, "xcrun", "devicectl", "device", "install", "app", "--device", *device, app); err != nil {
		return err
	}
	fmt.Printf("installed %s on %s, signed with %s\n", *bundleID, *device, profile.Name)
	return nil
}
