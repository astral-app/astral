package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func fuzzClang() (string, error) {
	candidates := []string{os.Getenv("ASTRAL_FUZZ_CLANG"), "/opt/homebrew/opt/llvm/bin/clang", "/usr/local/opt/llvm/bin/clang"}
	if path, err := exec.LookPath("clang"); err == nil {
		candidates = append(candidates, path)
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return "", err
	}
	defer cleanup()
	probe := filepath.Join(work, "probe.c")
	if err := os.WriteFile(probe, []byte("#include <stddef.h>\n#include <stdint.h>\nint LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) { (void)data; (void)size; return 0; }\n"), 0o644); err != nil {
		return "", err
	}
	for _, candidate := range candidates {
		if candidate == "" || !fileExists(candidate) {
			continue
		}
		if exec.Command(candidate, "-fsanitize=fuzzer", probe, "-o", filepath.Join(work, "probe")).Run() == nil {
			return candidate, nil
		}
	}
	return "", errors.New("no clang with libFuzzer; install LLVM (brew install llvm) or set ASTRAL_FUZZ_CLANG")
}

func generateSeeds(work string, corpus string) error {
	generator := filepath.Join(work, "seeds")
	tests := repoPath("packages", "ios", "tests")
	sources, err := wireSources()
	if err != nil {
		return err
	}
	if err := run(tests, "cc", append([]string{"-O1", "-w", "-o", generator, "fuzz/seeds.c"}, append(sources, "-lm")...)...); err != nil {
		return err
	}
	if err := os.MkdirAll(corpus, 0o755); err != nil {
		return err
	}
	return run(tests, generator, corpus)
}

func runFuzz(args []string) error {
	flags := newFlags("fuzz")
	seconds := flags.Int("seconds", 60, "how long to fuzz")
	corpus := flags.String("corpus", "", "corpus directory to keep between runs")
	if err := flags.Parse(args); err != nil {
		return err
	}
	clang, err := fuzzClang()
	if err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	if *corpus == "" {
		*corpus = filepath.Join(work, "corpus")
	}
	if err := generateSeeds(work, *corpus); err != nil {
		return err
	}
	target := filepath.Join(work, "rewrite-fuzz")
	tests := repoPath("packages", "ios", "tests")
	sources, err := wireSources()
	if err != nil {
		return err
	}
	if err := run(tests, clang, append([]string{"-g", "-O1", "-fsanitize=fuzzer,address,undefined", "-fno-sanitize-recover=all", "-o", target, "fuzz/rewrite-fuzz.c"}, append(sources, "-lm")...)...); err != nil {
		return err
	}
	artifacts := filepath.Join(work, "artifacts") + string(os.PathSeparator)
	if err := os.MkdirAll(artifacts, 0o755); err != nil {
		return err
	}
	fuzzErr := run(work, target, fmt.Sprintf("-max_total_time=%d", *seconds), "-artifact_prefix="+artifacts, "-print_final_stats=1", *corpus)
	crashes, _ := filepath.Glob(artifacts + "*")
	if len(crashes) > 0 {
		kept := repoPath("fuzz-crashes")
		if err := os.MkdirAll(kept, 0o755); err != nil {
			return err
		}
		for _, crash := range crashes {
			copyFile(crash, filepath.Join(kept, filepath.Base(crash)))
		}
		return fmt.Errorf("the rewriter crashed on %d input(s), saved in fuzz-crashes/: %s", len(crashes), strings.Join(crashes, ", "))
	}
	return fuzzErr
}

func runBench(args []string) error {
	flags := newFlags("bench")
	if err := flags.Parse(args); err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	binary := filepath.Join(work, "rewrite-bench")
	tests := repoPath("packages", "ios", "tests")
	library, err := rustHostReleaseLibrary()
	if err != nil {
		return err
	}
	sources := append([]string{"-I" + repoPath("packages", "core", "wire")}, wireSourcePaths()...)
	if err := run(tests, "cc", append([]string{"-O2", "-flto", "-w", "-o", binary, "bench/rewrite-bench.c"}, append(sources, library, "-lm")...)...); err != nil {
		return err
	}
	return run(repoPath(), binary, flags.Args()...)
}
