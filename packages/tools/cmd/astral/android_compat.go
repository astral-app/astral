package main

import (
	"archive/zip"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

const maximumAndroidDexSize = 128 << 20

type androidCompatResult struct {
	Channel              string          `json:"channel,omitempty"`
	VersionCode          int             `json:"versionCode,omitempty"`
	APKs                 []string        `json:"apks"`
	Phoenix              bool            `json:"phoenix"`
	Supported            bool            `json:"supported"`
	DexFiles             []string        `json:"dexFiles"`
	Classes              map[string]bool `json:"classes"`
	Methods              map[string]bool `json:"methods"`
	PropertiesFound      int             `json:"propertiesFound"`
	PropertiesExpected   int             `json:"propertiesExpected"`
	MissingProperties    []string        `json:"missingProperties"`
	Events               map[string]bool `json:"events"`
	OptionalNetworkHooks map[string]bool `json:"optionalNetworkHooks"`
}

type androidAPKList []string

func (files *androidAPKList) Set(value string) error {
	if strings.TrimSpace(value) == "" {
		return errors.New("APK path cannot be empty")
	}
	*files = append(*files, value)
	return nil
}

func (files *androidAPKList) String() string {
	return strings.Join(*files, ",")
}

type dexMethod struct {
	classDescriptor string
	name            string
	signature       string
}

type dexIndex struct {
	strings         map[string]bool
	classes         map[string]bool
	declaredMethods []dexMethod
}

func readDexUleb(data []byte, offset *uint32) (uint32, error) {
	var value uint32
	for shift := uint(0); shift < 35; shift += 7 {
		if uint64(*offset) >= uint64(len(data)) {
			return 0, errors.New("DEX variable-length integer is truncated")
		}
		current := data[*offset]
		*offset = *offset + 1
		value |= uint32(current&0x7f) << shift
		if current&0x80 == 0 {
			return value, nil
		}
	}
	return 0, errors.New("DEX variable-length integer is invalid")
}

func dexRange(data []byte, offset uint32, count uint32, stride uint32) ([]byte, error) {
	end := uint64(offset) + uint64(count)*uint64(stride)
	if offset == 0 && count == 0 {
		return data[:0], nil
	}
	if end > uint64(len(data)) {
		return nil, errors.New("DEX index section is out of bounds")
	}
	return data[offset:uint32(end)], nil
}

func parseDex(data []byte) (dexIndex, error) {
	result := dexIndex{strings: map[string]bool{}, classes: map[string]bool{}}
	if len(data) < 112 || string(data[:4]) != "dex\n" || data[7] != 0 {
		return result, errors.New("APK contains an invalid DEX file")
	}
	if binary.LittleEndian.Uint32(data[40:44]) != 0x12345678 {
		return result, errors.New("APK contains a DEX file with an unsupported byte order")
	}
	if binary.LittleEndian.Uint32(data[32:36]) != uint32(len(data)) || binary.LittleEndian.Uint32(data[36:40]) != 112 {
		return result, errors.New("APK contains a DEX file with an invalid header")
	}
	stringCount := binary.LittleEndian.Uint32(data[56:60])
	stringOffset := binary.LittleEndian.Uint32(data[60:64])
	typeCount := binary.LittleEndian.Uint32(data[64:68])
	typeOffset := binary.LittleEndian.Uint32(data[68:72])
	protoCount := binary.LittleEndian.Uint32(data[72:76])
	protoOffset := binary.LittleEndian.Uint32(data[76:80])
	fieldCount := binary.LittleEndian.Uint32(data[80:84])
	fieldOffset := binary.LittleEndian.Uint32(data[84:88])
	methodCount := binary.LittleEndian.Uint32(data[88:92])
	methodOffset := binary.LittleEndian.Uint32(data[92:96])
	classCount := binary.LittleEndian.Uint32(data[96:100])
	classOffset := binary.LittleEndian.Uint32(data[100:104])
	if stringCount > 1<<22 || typeCount > 1<<22 || protoCount > 1<<22 || fieldCount > 1<<22 || methodCount > 1<<22 || classCount > 1<<22 {
		return result, errors.New("DEX index table exceeds the scanner limit")
	}
	stringIDs, err := dexRange(data, stringOffset, stringCount, 4)
	if err != nil {
		return result, err
	}
	typeIDs, err := dexRange(data, typeOffset, typeCount, 4)
	if err != nil {
		return result, err
	}
	protoIDs, err := dexRange(data, protoOffset, protoCount, 12)
	if err != nil {
		return result, err
	}
	if _, err := dexRange(data, fieldOffset, fieldCount, 8); err != nil {
		return result, err
	}
	methodIDs, err := dexRange(data, methodOffset, methodCount, 8)
	if err != nil {
		return result, err
	}
	classDefs, err := dexRange(data, classOffset, classCount, 32)
	if err != nil {
		return result, err
	}
	stringsByIndex := make([]string, stringCount)
	for index := uint32(0); index < stringCount; index++ {
		itemOffset := binary.LittleEndian.Uint32(stringIDs[index*4 : index*4+4])
		if itemOffset >= uint32(len(data)) {
			return result, errors.New("DEX string offset is out of bounds")
		}
		cursor := itemOffset
		if _, err := readDexUleb(data, &cursor); err != nil {
			return result, err
		}
		end := cursor
		for end < uint32(len(data)) && data[end] != 0 {
			end++
		}
		if end == uint32(len(data)) {
			return result, errors.New("DEX string is not terminated")
		}
		value := string(data[cursor:end])
		stringsByIndex[index] = value
		result.strings[value] = true
	}
	typeDescriptors := make([]string, typeCount)
	for index := uint32(0); index < typeCount; index++ {
		stringIndex := binary.LittleEndian.Uint32(typeIDs[index*4 : index*4+4])
		if stringIndex >= stringCount {
			return result, errors.New("DEX type has an invalid descriptor index")
		}
		typeDescriptors[index] = stringsByIndex[stringIndex]
	}
	protoSignatures := make([]string, protoCount)
	for index := uint32(0); index < protoCount; index++ {
		entry := protoIDs[index*12 : index*12+12]
		returnType := binary.LittleEndian.Uint32(entry[4:8])
		parametersOffset := binary.LittleEndian.Uint32(entry[8:12])
		if returnType >= typeCount {
			return result, errors.New("DEX method prototype has an invalid return type")
		}
		var parameters strings.Builder
		parameters.WriteByte('(')
		if parametersOffset != 0 {
			if uint64(parametersOffset)+4 > uint64(len(data)) {
				return result, errors.New("DEX parameter list is out of bounds")
			}
			count := binary.LittleEndian.Uint32(data[parametersOffset : parametersOffset+4])
			if count > 1<<16 || uint64(parametersOffset)+4+uint64(count)*2 > uint64(len(data)) {
				return result, errors.New("DEX parameter list is invalid")
			}
			for parameter := uint32(0); parameter < count; parameter++ {
				typeIndex := binary.LittleEndian.Uint16(data[parametersOffset+4+parameter*2 : parametersOffset+6+parameter*2])
				if uint32(typeIndex) >= typeCount {
					return result, errors.New("DEX method parameter has an invalid type")
				}
				parameters.WriteString(typeDescriptors[typeIndex])
			}
		}
		parameters.WriteByte(')')
		parameters.WriteString(typeDescriptors[returnType])
		protoSignatures[index] = parameters.String()
	}
	methodTable := make([]dexMethod, methodCount)
	for index := uint32(0); index < methodCount; index++ {
		entry := methodIDs[index*8 : index*8+8]
		classIndex := binary.LittleEndian.Uint16(entry[0:2])
		protoIndex := binary.LittleEndian.Uint16(entry[2:4])
		nameIndex := binary.LittleEndian.Uint32(entry[4:8])
		if uint32(classIndex) >= typeCount || uint32(protoIndex) >= protoCount || nameIndex >= stringCount {
			return result, errors.New("DEX method has an invalid index")
		}
		methodTable[index] = dexMethod{typeDescriptors[classIndex], stringsByIndex[nameIndex], protoSignatures[protoIndex]}
	}
	for index := uint32(0); index < classCount; index++ {
		entry := classDefs[index*32 : index*32+32]
		classIndex := binary.LittleEndian.Uint32(entry[0:4])
		classDataOffset := binary.LittleEndian.Uint32(entry[24:28])
		if classIndex >= typeCount {
			return result, errors.New("DEX class has an invalid type index")
		}
		classDescriptor := typeDescriptors[classIndex]
		result.classes[classDescriptor] = true
		if classDataOffset == 0 {
			continue
		}
		cursor := classDataOffset
		staticFields, readErr := readDexUleb(data, &cursor)
		if readErr != nil {
			return result, readErr
		}
		instanceFields, readErr := readDexUleb(data, &cursor)
		if readErr != nil {
			return result, readErr
		}
		directMethods, readErr := readDexUleb(data, &cursor)
		if readErr != nil {
			return result, readErr
		}
		virtualMethods, readErr := readDexUleb(data, &cursor)
		if readErr != nil {
			return result, readErr
		}
		if uint64(staticFields)+uint64(instanceFields) > uint64(fieldCount) || uint64(directMethods)+uint64(virtualMethods) > uint64(methodCount) {
			return result, errors.New("DEX class data contains an invalid member count")
		}
		for _, count := range []uint32{staticFields, instanceFields} {
			for field := uint32(0); field < count; field++ {
				if _, readErr = readDexUleb(data, &cursor); readErr != nil {
					return result, readErr
				}
				if _, readErr = readDexUleb(data, &cursor); readErr != nil {
					return result, readErr
				}
			}
		}
		for _, count := range []uint32{directMethods, virtualMethods} {
			methodIndex := uint32(0)
			for method := uint32(0); method < count; method++ {
				difference, methodErr := readDexUleb(data, &cursor)
				if methodErr != nil {
					return result, methodErr
				}
				methodIndex += difference
				if _, methodErr = readDexUleb(data, &cursor); methodErr != nil {
					return result, methodErr
				}
				if _, methodErr = readDexUleb(data, &cursor); methodErr != nil {
					return result, methodErr
				}
				if methodIndex >= uint32(len(methodTable)) {
					return result, errors.New("DEX class method has an invalid index")
				}
				result.declaredMethods = append(result.declaredMethods, methodTable[methodIndex])
			}
		}
	}
	return result, nil
}

func dexEntries(archive *zip.ReadCloser) ([]string, error) {
	var names []string
	for _, file := range archive.File {
		name := path.Base(file.Name)
		if (name != "classes.dex" && !strings.HasPrefix(name, "classes")) || !strings.HasSuffix(name, ".dex") {
			continue
		}
		if name != "classes.dex" {
			middle := strings.TrimSuffix(strings.TrimPrefix(name, "classes"), ".dex")
			if number, err := strconv.Atoi(middle); err != nil || number < 2 {
				continue
			}
		}
		if file.UncompressedSize64 > maximumAndroidDexSize {
			return nil, fmt.Errorf("%s exceeds the DEX scanner size limit", name)
		}
		names = append(names, file.Name)
	}
	sort.Strings(names)
	if len(names) == 0 {
		return nil, nil
	}
	return names, nil
}

func androidProperties() ([]string, error) {
	data, err := os.ReadFile(repoPath("packages", "core", "wire", "PhoenixProperties.h"))
	if err != nil {
		return nil, err
	}
	pattern := regexp.MustCompile(`X\(([A-Za-z_][A-Za-z0-9_]*),\s*[0-9]+\)`)
	matches := pattern.FindAllSubmatch(data, -1)
	properties := make([]string, 0, len(matches))
	for _, match := range matches {
		properties = append(properties, string(match[1]))
	}
	if len(properties) == 0 {
		return nil, errors.New("could not read astral's Phoenix property list")
	}
	return properties, nil
}

func scanAndroidAPK(apk string, properties []string) (androidCompatResult, error) {
	return scanAndroidAPKs([]string{apk}, properties)
}

func scanAndroidAPKs(apks []string, properties []string) (androidCompatResult, error) {
	result := androidCompatResult{
		APKs:                 []string{},
		Classes:              map[string]bool{},
		Methods:              map[string]bool{},
		MissingProperties:    []string{},
		Events:               map[string]bool{},
		OptionalNetworkHooks: map[string]bool{},
	}
	combined := dexIndex{strings: map[string]bool{}, classes: map[string]bool{}}
	for _, apk := range apks {
		archive, err := zip.OpenReader(apk)
		if err != nil {
			return result, err
		}
		names, err := dexEntries(archive)
		if err != nil {
			archive.Close()
			return result, err
		}
		if len(names) == 0 {
			archive.Close()
			continue
		}
		result.APKs = append(result.APKs, path.Base(apk))
		for _, name := range names {
			var entry *zip.File
			for _, candidate := range archive.File {
				if candidate.Name == name {
					entry = candidate
					break
				}
			}
			if entry == nil {
				archive.Close()
				return result, errors.New("DEX entry disappeared while scanning the APK")
			}
			file, openErr := entry.Open()
			if openErr != nil {
				archive.Close()
				return result, openErr
			}
			data, readErr := io.ReadAll(io.LimitReader(file, maximumAndroidDexSize+1))
			closeErr := file.Close()
			if readErr != nil {
				archive.Close()
				return result, readErr
			}
			if closeErr != nil {
				archive.Close()
				return result, closeErr
			}
			if len(data) > maximumAndroidDexSize {
				archive.Close()
				return result, fmt.Errorf("%s exceeds the DEX scanner size limit", path.Base(name))
			}
			parsed, parseErr := parseDex(data)
			if parseErr != nil {
				archive.Close()
				return result, fmt.Errorf("%s: %w", path.Base(name), parseErr)
			}
			result.DexFiles = append(result.DexFiles, path.Base(apk)+":"+path.Base(name))
			for value := range parsed.strings {
				combined.strings[value] = true
			}
			for value := range parsed.classes {
				combined.classes[value] = true
			}
			combined.declaredMethods = append(combined.declaredMethods, parsed.declaredMethods...)
		}
		archive.Close()
	}
	if len(result.DexFiles) == 0 {
		return result, errors.New("the supplied APKs contain no DEX files")
	}
	result.Phoenix = combined.classes["Lcom/discord/nativeapp/bridge/GuestHost;"] && combined.classes["Lcom/discord/nativeapp/MainActivity;"]
	result.Classes["GuestHost"] = combined.classes["Lcom/discord/nativeapp/bridge/GuestHost;"]
	result.Classes["MainActivity"] = combined.classes["Lcom/discord/nativeapp/MainActivity;"]
	for _, method := range combined.declaredMethods {
		if method.classDescriptor != "Lcom/discord/nativeapp/bridge/GuestHost;" {
			continue
		}
		if method.name == "applyOps" && strings.HasPrefix(method.signature, "([BI)") {
			result.Methods["GuestHost.applyOps([B, int)"] = true
		}
		if method.signature == "([BLkotlin/jvm/functions/Function0;)V" {
			result.Methods["GuestHost event callback"] = true
		}
	}
	for _, name := range []string{"onResume", "onPause", "onActivityResult"} {
		for _, method := range combined.declaredMethods {
			if method.classDescriptor != "Lcom/discord/nativeapp/MainActivity;" || method.name != name {
				continue
			}
			if name == "onActivityResult" && method.signature != "(IILandroid/content/Intent;)V" {
				continue
			}
			if name != "onActivityResult" && method.signature != "()V" {
				continue
			}
			result.Methods["MainActivity."+name] = true
		}
	}
	for _, property := range properties {
		if combined.strings[property] {
			result.PropertiesFound++
		} else {
			result.MissingProperties = append(result.MissingProperties, property)
		}
	}
	sort.Strings(result.MissingProperties)
	result.PropertiesExpected = len(properties)
	result.Events["press"] = combined.strings["press"]
	result.Events["change"] = combined.strings["change"]
	result.OptionalNetworkHooks["Cronet"] = combined.classes["Lorg/chromium/net/CronetEngine$Builder;"]
	result.OptionalNetworkHooks["Phoenix shaded OkHttp"] = combined.classes["Lg90/h0;"] && combined.classes["Lg90/i0;"]
	result.Supported = result.Phoenix && result.Methods["GuestHost.applyOps([B, int)"] && result.Methods["GuestHost event callback"] &&
		result.PropertiesExpected == result.PropertiesFound && result.Events["press"] && result.Events["change"]
	return result, nil
}

func androidCompatReport(result androidCompatResult) string {
	status := "supported"
	if !result.Supported {
		status = "**not supported**"
	}
	var report strings.Builder
	fmt.Fprintf(&report, "## astral on Android Phoenix\n\nThis APK is %s.\n\n", status)
	if result.Channel != "" {
		fmt.Fprintf(&report, "Discord %s, version code %d, signed by Discord.\n\n", result.Channel, result.VersionCode)
	}
	fmt.Fprintf(&report, "Scanned APKs: `%s`.\n\n", strings.Join(result.APKs, "`, `"))
	fmt.Fprintf(&report, "Scanned DEX files: `%s`.\n\n", strings.Join(result.DexFiles, "`, `"))
	report.WriteString("| Check | Result |\n| --- | --- |\n")
	fmt.Fprintf(&report, "| Phoenix host classes | GuestHost %t, MainActivity %t |\n", result.Classes["GuestHost"], result.Classes["MainActivity"])
	fmt.Fprintf(&report, "| `GuestHost.applyOps(byte[], int)` | %t |\n", result.Methods["GuestHost.applyOps([B, int)"])
	fmt.Fprintf(&report, "| GuestHost event callback | %t |\n", result.Methods["GuestHost event callback"])
	fmt.Fprintf(&report, "| MainActivity lifecycle/document hooks | resume %t, pause %t, activity result %t |\n",
		result.Methods["MainActivity.onResume"], result.Methods["MainActivity.onPause"], result.Methods["MainActivity.onActivityResult"])
	fmt.Fprintf(&report, "| Phoenix property names | %d / %d |\n", result.PropertiesFound, result.PropertiesExpected)
	fmt.Fprintf(&report, "| Event identifiers | press %t, change %t |\n", result.Events["press"], result.Events["change"])
	fmt.Fprintf(&report, "| Optional network hooks | Cronet %t, Phoenix shaded OkHttp %t |\n", result.OptionalNetworkHooks["Cronet"], result.OptionalNetworkHooks["Phoenix shaded OkHttp"])
	if len(result.MissingProperties) > 0 {
		fmt.Fprintf(&report, "\nMissing Phoenix properties: `%s`.\n", strings.Join(result.MissingProperties, "`, `"))
	}
	return report.String()
}

func runAndroidCompat(args []string) error {
	flags := newFlags("android-compat")
	var apks androidAPKList
	flags.Var(&apks, "apk", "Discord APK or split APK; repeat this flag for each file")
	channel := flags.String("channel", "", "download and verify the latest Discord APKs of this channel (stable, beta or alpha) instead of passing --apk")
	resultsPath := flags.String("results", "android-compat.json", "where to write the results")
	reportPath := flags.String("report", "android-compat.md", "where to write a Markdown report")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if (len(apks) == 0) == (*channel == "") {
		return errors.New("usage: astral android-compat (--channel <stable|beta|alpha> | --apk <Discord.apk> [--apk <split.apk> ...]) [--results android-compat.json] [--report android-compat.md]")
	}
	versionCode := 0
	if *channel != "" {
		work, cleanup, err := temporaryDirectory()
		if err != nil {
			return err
		}
		defer cleanup()
		if versionCode, apks, err = downloadDiscordAPKs(*channel, work); err != nil {
			return err
		}
	}
	properties, err := androidProperties()
	if err != nil {
		return err
	}
	result, err := scanAndroidAPKs(apks, properties)
	if err != nil {
		return err
	}
	result.Channel = *channel
	result.VersionCode = versionCode
	if *channel != "" {
		for index, apk := range result.APKs {
			result.APKs[index] = filepath.Base(apk)
		}
	}
	encoded, err := json.MarshalIndent(result, "", "\t")
	if err != nil {
		return err
	}
	if err := os.WriteFile(*resultsPath, append(encoded, '\n'), 0o644); err != nil {
		return err
	}
	report := androidCompatReport(result)
	if *reportPath != "" {
		if err := os.WriteFile(*reportPath, []byte(report), 0o644); err != nil {
			return err
		}
	}
	fmt.Print(report)
	if !result.Supported {
		return errors.New("this Android Discord build is not supported")
	}
	return nil
}
