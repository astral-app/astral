package main

import (
	"fmt"
	"os"
	"sort"
	"strings"
)

type command struct {
	usage string
	run   func(args []string) error
}

var commands = map[string]command{
	"android":       {"android <build|deploy> ...", runAndroid},
	"test":          {"test [--phoenix <PhoenixFatClient>]", runTest},
	"lint":          {"lint", runLint},
	"format":        {"format", runFormat},
	"sign":          {"sign [--key <pem>]", runSign},
	"stage":         {"stage <directory>", runStage},
	"embed-prelude": {"embed-prelude <prelude.lua> <header>", runEmbedPrelude},
	"deploy":        {"deploy [--device <udid>]", runDeploy},
	"build-ipa":     {"build-ipa --ipa <decrypted Discord.ipa> [--deb <astral.deb>] [--output <astral.ipa>]", runBuildIPA},
	"open-settings": {"open-settings [--device <udid>] [row]", runOpenSettings},
	"prepare-theos": {"prepare-theos [--theos <directory>]", runPrepareTheos},
	"wire-dump":     {"wire-dump [--device <udid>] <output directory>", runWireDump},
	"dev":           {"dev <status|push|watch|eval|logs|theme|backup> ...", runDev},
	"fuzz":          {"fuzz [--seconds 60] [--corpus <directory>]", runFuzz},
	"bench":         {"bench [captured batch ...]", runBench},
	"create":        {"create [--name <name>] [--author <name>] [--dir <directory>] <id>", runCreate},
	"sideload":      {"sideload --ipa <IPA> --bundle-id <id> [--device <udid>] [--profile <mobileprovision>]", runSideload},
	"palette":       {"palette semantic --source <generated-definitions.tsx> --origin <description> [--output <toml>]", runPalette},
	"vphone":        {"vphone <boot|status|install|launch|shell|push|pull|deploy> ...", runVphone},
	"sdk":           {"sdk install", runSDK},
	"doctor":        {"doctor", runDoctor},
	"replay":        {"replay <plugin>... <capture>", runReplay},
	"check":         {"check [--staged] [file ...]", runCheck},
	"android-core":  {"android-core [--ndk <directory>] [--output <directory>]", runAndroidCore},
	"compat":        {"compat --ipa <decrypted Discord.ipa> [--channel stable] [--baseline compat.json] [--results compat.json] [--report report.md]", runCompat},
}

func usage() {
	names := make([]string, 0, len(commands))
	for name := range commands {
		names = append(names, name)
	}
	sort.Strings(names)
	lines := []string{"usage: astral <command>"}
	for _, name := range names {
		lines = append(lines, "  "+commands[name].usage)
	}
	fmt.Fprintln(os.Stderr, strings.Join(lines, "\n"))
	os.Exit(2)
}

func main() {
	if len(os.Args) < 2 {
		usage()
	}
	selected, found := commands[os.Args[1]]
	if !found {
		usage()
	}
	if err := selected.run(os.Args[2:]); err != nil {
		fmt.Fprintln(os.Stderr, "astral "+os.Args[1]+": "+err.Error())
		os.Exit(1)
	}
}
