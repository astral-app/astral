package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
)

func remoteHead(repository string) (string, error) {
	listing, err := output(repoPath(), "git", "ls-remote", repository, "HEAD")
	if err != nil {
		return "", err
	}
	fields := strings.Fields(listing)
	if len(fields) == 0 {
		return "", errors.New("no HEAD for " + repository)
	}
	return fields[0], nil
}

func updateTheos(theos string, head string) error {
	if !fileExists(theos) {
		return run(repoPath(), "git", "clone", "--recursive", "https://github.com/theos/theos", theos)
	}
	current, err := output(theos, "git", "rev-parse", "HEAD")
	if err != nil {
		return err
	}
	if strings.TrimSpace(current) == head {
		return nil
	}
	if err := run(theos, "git", "pull", "--ff-only", "--recurse-submodules"); err != nil {
		return err
	}
	return run(theos, "git", "submodule", "update", "--init", "--recursive")
}

func updateSDKs(theos string, head string) error {
	revisionFile := filepath.Join(theos, "sdks", ".revision")
	if revision, err := os.ReadFile(revisionFile); err == nil && strings.TrimSpace(string(revision)) == head {
		return nil
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	clone := filepath.Join(work, "sdks")
	if err := run(work, "git", "clone", "--depth", "1", "https://github.com/theos/sdks", clone); err != nil {
		return err
	}
	old, err := filepath.Glob(filepath.Join(theos, "sdks", "*.sdk"))
	if err != nil {
		return err
	}
	for _, sdk := range old {
		if err := os.RemoveAll(sdk); err != nil {
			return err
		}
	}
	fresh, err := filepath.Glob(filepath.Join(clone, "*.sdk"))
	if err != nil {
		return err
	}
	for _, sdk := range fresh {
		if err := os.Rename(sdk, filepath.Join(theos, "sdks", filepath.Base(sdk))); err != nil {
			return err
		}
	}
	return os.WriteFile(revisionFile, []byte(head+"\n"), 0o644)
}

func runPrepareTheos(args []string) error {
	flags := newFlags("prepare-theos")
	theos := flags.String("theos", os.Getenv("THEOS"), "Theos checkout")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *theos == "" {
		return errors.New("set --theos or THEOS to the Theos checkout")
	}
	theosHead, err := remoteHead("https://github.com/theos/theos")
	if err != nil {
		return err
	}
	sdksHead, err := remoteHead("https://github.com/theos/sdks")
	if err != nil {
		return err
	}
	if err := run(repoPath(), "brew", "install", "ldid", "make"); err != nil {
		return err
	}
	if err := updateTheos(*theos, theosHead); err != nil {
		return err
	}
	return updateSDKs(*theos, sdksHead)
}
