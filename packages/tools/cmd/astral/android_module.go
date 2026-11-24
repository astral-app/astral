package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const androidUsage = `usage: astral android <command>
  build [--java-home <JDK>] [--output <directory>] [--patcher <LSPatch.jar> <Discord APK>...]
  deploy --device <adb serial> --mode <module|sideload> <APK>...
  download [--channel alpha] [--output <directory>]`

func runAndroid(args []string) error {
	if len(args) == 0 {
		return errors.New(androidUsage)
	}
	switch args[0] {
	case "build":
		return buildAndroidModule(args[1:])
	case "deploy":
		return deployAndroidModule(args[1:])
	case "download":
		return runAndroidDownload(args[1:])
	default:
		return errors.New(androidUsage)
	}
}

func androidJavaHome(explicit string) (string, error) {
	candidates := []string{explicit, os.Getenv("JAVA_HOME")}
	if explicit == "" {
		home, _ := os.UserHomeDir()
		cached, _ := filepath.Glob(filepath.Join(home, "Library", "Caches", "unbound", "jdk", "*", "Contents", "Home"))
		sort.Sort(sort.Reverse(sort.StringSlice(cached)))
		candidates = append(candidates, cached...)
	}
	for _, candidate := range candidates {
		if candidate != "" && fileExists(filepath.Join(candidate, "bin", "java")) {
			return filepath.Abs(candidate)
		}
		if explicit != "" {
			break
		}
	}
	return "", errors.New("no Java runtime; pass --java-home or set JAVA_HOME to a JDK 21 installation")
}

func androidAPKInputs(paths []string) ([]string, error) {
	var files []string
	seen := map[string]bool{}
	for _, input := range paths {
		absolute, err := filepath.Abs(input)
		if err != nil {
			return nil, err
		}
		info, err := os.Stat(absolute)
		if err != nil {
			return nil, err
		}
		if !info.Mode().IsRegular() || !strings.EqualFold(filepath.Ext(absolute), ".apk") || seen[absolute] {
			return nil, fmt.Errorf("expected distinct APK files: %s", input)
		}
		seen[absolute] = true
		files = append(files, absolute)
	}
	return files, nil
}

func buildAndroidModule(args []string) error {
	flags := newFlags("android build")
	java := flags.String("java-home", "", "JDK 21 home")
	destination := flags.String("output", repoPath("packages", "android", "build", "astral"), "output directory")
	patcher := flags.String("patcher", "", "LSPatch JAR for a rootless install")
	if err := flags.Parse(args); err != nil {
		return err
	}
	inputs, err := androidAPKInputs(flags.Args())
	if err != nil {
		return err
	}
	if (*patcher == "") != (len(inputs) == 0) {
		return errors.New("pass --patcher and the Discord base APK with its splits together")
	}
	javaHome, err := androidJavaHome(*java)
	if err != nil {
		return err
	}
	environment := []string{"JAVA_HOME=" + javaHome}
	if os.Getenv("ANDROID_HOME") == "" {
		home, _ := os.UserHomeDir()
		sdk := filepath.Join(home, "Library", "Android", "sdk")
		if fileExists(sdk) {
			environment = append(environment, "ANDROID_HOME="+sdk)
		}
	}
	if err := runWithEnv(repoPath("packages", "android"), environment, "./gradlew", ":module:assembleDebug"); err != nil {
		return err
	}
	module := repoPath("packages", "android", "module", "build", "outputs", "apk", "debug", "module-debug.apk")
	output, err := filepath.Abs(*destination)
	if err != nil {
		return err
	}
	if err := copyFile(module, filepath.Join(output, "astral-module.apk")); err != nil {
		return err
	}
	if len(inputs) == 0 {
		fmt.Println(filepath.Join(output, "astral-module.apk"))
		return nil
	}
	jar, err := filepath.Abs(*patcher)
	if err != nil {
		return err
	}
	work, cleanup, err := temporaryDirectory()
	if err != nil {
		return err
	}
	defer cleanup()
	patched := filepath.Join(work, "patched")
	arguments := append([]string{"-jar", jar}, inputs...)
	arguments = append(arguments, "-m", module, "-l", "2", "-o", patched)
	if err := run(work, filepath.Join(javaHome, "bin", "java"), arguments...); err != nil {
		return err
	}
	files, err := filepath.Glob(filepath.Join(patched, "*.apk"))
	if err != nil {
		return err
	}
	if len(files) != len(inputs) {
		return fmt.Errorf("patcher produced %d APKs for %d inputs", len(files), len(inputs))
	}
	for _, file := range files {
		target := filepath.Join(output, "sideload", filepath.Base(file))
		if err := copyFile(file, target); err != nil {
			return err
		}
		fmt.Println(target)
	}
	return nil
}

func deployAndroidModule(args []string) error {
	flags := newFlags("android deploy")
	serial := flags.String("device", "", "explicit adb device serial")
	mode := flags.String("mode", "", "module or sideload")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *serial == "" || (*mode != "module" && *mode != "sideload") || flags.NArg() == 0 {
		return errors.New(androidUsage)
	}
	inputs, err := androidAPKInputs(flags.Args())
	if err != nil {
		return err
	}
	if *mode == "module" && len(inputs) != 1 {
		return errors.New("module deployment takes exactly one astral module APK")
	}
	install := append([]string{"install-multiple", "-r"}, inputs...)
	if err := run(repoPath(), "adb", adbArguments(*serial, install...)...); err != nil {
		return err
	}
	if *mode == "module" {
		for _, command := range []string{
			"sh /data/adb/modules/zygisk_vector/cli modules enable app.astral.android",
			"sh /data/adb/modules/zygisk_vector/cli scope set app.astral.android com.discord/0",
		} {
			if err := run(repoPath(), "adb", adbArguments(*serial, "shell", "su", "-c", command)...); err != nil {
				return err
			}
		}
	}
	if err := run(repoPath(), "adb", adbArguments(*serial, "shell", "am", "force-stop", "com.discord")...); err != nil {
		return err
	}
	return run(repoPath(), "adb", adbArguments(*serial, "shell", "monkey", "-p", "com.discord", "-c", "android.intent.category.LAUNCHER", "1")...)
}
