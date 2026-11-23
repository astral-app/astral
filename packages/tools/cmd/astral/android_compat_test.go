package main

import (
	"archive/zip"
	"encoding/binary"
	"os"
	"path/filepath"
	"sort"
	"testing"
)

type androidDexTestProto struct {
	parameters []string
	result     string
}

type androidDexTestMethod struct {
	class     string
	name      string
	prototype androidDexTestProto
}

func appendDexUleb(data []byte, value uint32) []byte {
	for value >= 0x80 {
		data = append(data, byte(value)|0x80)
		value >>= 7
	}
	return append(data, byte(value))
}

func testAndroidDex() []byte {
	guestHost := "Lcom/discord/nativeapp/bridge/GuestHost;"
	mainActivity := "Lcom/discord/nativeapp/MainActivity;"
	function0 := "Lkotlin/jvm/functions/Function0;"
	intent := "Landroid/content/Intent;"
	byteArray := "[B"
	integer := "I"
	void := "V"
	methods := []androidDexTestMethod{
		{guestHost, "applyOps", androidDexTestProto{[]string{byteArray, integer}, void}},
		{guestHost, "a", androidDexTestProto{[]string{byteArray, function0}, void}},
		{mainActivity, "onActivityResult", androidDexTestProto{[]string{integer, integer, intent}, void}},
		{mainActivity, "onPause", androidDexTestProto{nil, void}},
		{mainActivity, "onResume", androidDexTestProto{nil, void}},
	}
	stringsSet := map[string]bool{
		guestHost: true, mainActivity: true, function0: true, intent: true, byteArray: true, integer: true, void: true,
		"applyOps": true, "a": true, "onActivityResult": true, "onPause": true, "onResume": true,
		"accessibilityActions": true, "text": true, "uri": true, "press": true, "change": true,
	}
	var stringsByIndex []string
	for value := range stringsSet {
		if stringsSet[value] {
			stringsByIndex = append(stringsByIndex, value)
		}
	}
	sort.Strings(stringsByIndex)
	stringIndexes := map[string]uint32{}
	for index, value := range stringsByIndex {
		stringIndexes[value] = uint32(index)
	}
	typeDescriptors := []string{guestHost, mainActivity, function0, intent, byteArray, integer, void}
	sort.Strings(typeDescriptors)
	typeIndexes := map[string]uint16{}
	for index, value := range typeDescriptors {
		typeIndexes[value] = uint16(index)
	}
	prototypes := []androidDexTestProto{
		{[]string{byteArray, integer}, void},
		{[]string{byteArray, function0}, void},
		{[]string{integer, integer, intent}, void},
		{nil, void},
	}
	prototypeKey := func(prototype androidDexTestProto) string {
		key := "(" + stringsJoin(prototype.parameters) + ")" + prototype.result
		return key
	}
	prototypeIndexes := map[string]uint16{}
	for index, prototype := range prototypes {
		prototypeIndexes[prototypeKey(prototype)] = uint16(index)
	}
	type indexedMethod struct {
		androidDexTestMethod
		index uint16
	}
	var indexedMethods []indexedMethod
	for _, method := range methods {
		indexedMethods = append(indexedMethods, indexedMethod{androidDexTestMethod: method})
	}
	sort.Slice(indexedMethods, func(left int, right int) bool {
		leftClass := typeIndexes[indexedMethods[left].class]
		rightClass := typeIndexes[indexedMethods[right].class]
		if leftClass != rightClass {
			return leftClass < rightClass
		}
		leftName := stringIndexes[indexedMethods[left].name]
		rightName := stringIndexes[indexedMethods[right].name]
		if leftName != rightName {
			return leftName < rightName
		}
		return prototypeIndexes[prototypeKey(indexedMethods[left].prototype)] < prototypeIndexes[prototypeKey(indexedMethods[right].prototype)]
	})
	for index := range indexedMethods {
		indexedMethods[index].index = uint16(index)
	}
	classDescriptors := []string{guestHost, mainActivity}
	sort.Slice(classDescriptors, func(left int, right int) bool {
		return typeIndexes[classDescriptors[left]] < typeIndexes[classDescriptors[right]]
	})
	const headerSize = 112
	stringOffset := uint32(headerSize)
	typeOffset := stringOffset + uint32(len(stringsByIndex))*4
	prototypeOffset := typeOffset + uint32(len(typeDescriptors))*4
	methodOffset := prototypeOffset + uint32(len(prototypes))*12
	classOffset := methodOffset + uint32(len(indexedMethods))*8
	dataOffset := classOffset + uint32(len(classDescriptors))*32
	data := make([]byte, 0)
	stringDataOffsets := make([]uint32, len(stringsByIndex))
	for index, value := range stringsByIndex {
		stringDataOffsets[index] = dataOffset + uint32(len(data))
		data = appendDexUleb(data, uint32(len(value)))
		data = append(data, []byte(value)...)
		data = append(data, 0)
	}
	parameterOffsets := make([]uint32, len(prototypes))
	for index, prototype := range prototypes {
		if len(data)%4 != 0 {
			data = append(data, make([]byte, 4-len(data)%4)...)
		}
		if len(prototype.parameters) == 0 {
			continue
		}
		parameterOffsets[index] = dataOffset + uint32(len(data))
		list := make([]byte, 4+2*len(prototype.parameters))
		binary.LittleEndian.PutUint32(list[0:4], uint32(len(prototype.parameters)))
		for parameter, descriptor := range prototype.parameters {
			binary.LittleEndian.PutUint16(list[4+parameter*2:6+parameter*2], typeIndexes[descriptor])
		}
		data = append(data, list...)
	}
	classDataOffsets := map[string]uint32{}
	for _, classDescriptor := range classDescriptors {
		classDataOffsets[classDescriptor] = dataOffset + uint32(len(data))
		var classMethods []uint16
		for _, method := range indexedMethods {
			if method.class == classDescriptor {
				classMethods = append(classMethods, method.index)
			}
		}
		data = appendDexUleb(data, 0)
		data = appendDexUleb(data, 0)
		data = appendDexUleb(data, uint32(len(classMethods)))
		data = appendDexUleb(data, 0)
		var previous uint16
		for index, methodIndex := range classMethods {
			difference := methodIndex
			if index > 0 {
				difference -= previous
			}
			data = appendDexUleb(data, uint32(difference))
			data = appendDexUleb(data, 1)
			data = appendDexUleb(data, 0)
			previous = methodIndex
		}
	}
	result := make([]byte, dataOffset+uint32(len(data)))
	copy(result[:8], []byte{'d', 'e', 'x', '\n', '0', '3', '9', 0})
	binary.LittleEndian.PutUint32(result[32:36], uint32(len(result)))
	binary.LittleEndian.PutUint32(result[36:40], headerSize)
	binary.LittleEndian.PutUint32(result[40:44], 0x12345678)
	binary.LittleEndian.PutUint32(result[56:60], uint32(len(stringsByIndex)))
	binary.LittleEndian.PutUint32(result[60:64], stringOffset)
	binary.LittleEndian.PutUint32(result[64:68], uint32(len(typeDescriptors)))
	binary.LittleEndian.PutUint32(result[68:72], typeOffset)
	binary.LittleEndian.PutUint32(result[72:76], uint32(len(prototypes)))
	binary.LittleEndian.PutUint32(result[76:80], prototypeOffset)
	binary.LittleEndian.PutUint32(result[88:92], uint32(len(indexedMethods)))
	binary.LittleEndian.PutUint32(result[92:96], methodOffset)
	binary.LittleEndian.PutUint32(result[96:100], uint32(len(classDescriptors)))
	binary.LittleEndian.PutUint32(result[100:104], classOffset)
	for index, offset := range stringDataOffsets {
		binary.LittleEndian.PutUint32(result[stringOffset+uint32(index)*4:stringOffset+uint32(index)*4+4], offset)
	}
	for index, descriptor := range typeDescriptors {
		binary.LittleEndian.PutUint32(result[typeOffset+uint32(index)*4:typeOffset+uint32(index)*4+4], stringIndexes[descriptor])
	}
	for index, prototype := range prototypes {
		entry := result[prototypeOffset+uint32(index)*12 : prototypeOffset+uint32(index)*12+12]
		binary.LittleEndian.PutUint32(entry[0:4], stringIndexes[prototypeKey(prototype)])
		binary.LittleEndian.PutUint32(entry[4:8], uint32(typeIndexes[prototype.result]))
		binary.LittleEndian.PutUint32(entry[8:12], parameterOffsets[index])
	}
	for index, method := range indexedMethods {
		entry := result[methodOffset+uint32(index)*8 : methodOffset+uint32(index)*8+8]
		binary.LittleEndian.PutUint16(entry[0:2], typeIndexes[method.class])
		binary.LittleEndian.PutUint16(entry[2:4], prototypeIndexes[prototypeKey(method.prototype)])
		binary.LittleEndian.PutUint32(entry[4:8], stringIndexes[method.name])
	}
	for index, descriptor := range classDescriptors {
		entry := result[classOffset+uint32(index)*32 : classOffset+uint32(index)*32+32]
		binary.LittleEndian.PutUint32(entry[0:4], uint32(typeIndexes[descriptor]))
		binary.LittleEndian.PutUint32(entry[24:28], classDataOffsets[descriptor])
	}
	copy(result[dataOffset:], data)
	return result
}

func stringsJoin(values []string) string {
	result := ""
	for _, value := range values {
		result += value
	}
	return result
}

func writeAndroidCompatAPK(t *testing.T, dex []byte) string {
	return writeAndroidAPK(t, filepath.Join(t.TempDir(), "Discord.apk"), "classes.dex", dex)
}

func writeAndroidAPK(t *testing.T, filename string, dexName string, dex []byte) string {
	t.Helper()
	file, err := os.Create(filename)
	if err != nil {
		t.Fatal(err)
	}
	archive := zip.NewWriter(file)
	entry, err := archive.Create(dexName)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := entry.Write(dex); err != nil {
		t.Fatal(err)
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	return filename
}

func TestAndroidCompatScansPhoenixClassesMethodsPropertiesAndEvents(t *testing.T) {
	apk := writeAndroidCompatAPK(t, testAndroidDex())
	result, err := scanAndroidAPK(apk, []string{"accessibilityActions", "text", "uri"})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Supported || result.PropertiesFound != 3 || !result.Events["press"] || !result.Events["change"] {
		t.Fatalf("unexpected Android compatibility result: %+v", result)
	}
	if !result.Methods["GuestHost.applyOps([B, int)"] || !result.Methods["GuestHost event callback"] {
		t.Fatalf("Phoenix hooks were not found: %+v", result.Methods)
	}
}

func TestAndroidCompatRejectsMissingRuntimeSurface(t *testing.T) {
	dex := testAndroidDex()
	index := stringsIndex(dex, []byte("press"))
	if index < 0 {
		t.Fatal("fixture does not contain its press event")
	}
	dex[index] = 'x'
	apk := writeAndroidCompatAPK(t, dex)
	result, err := scanAndroidAPK(apk, []string{"accessibilityActions", "text", "uri"})
	if err != nil {
		t.Fatal(err)
	}
	if result.Supported || result.Events["press"] {
		t.Fatalf("scanner accepted a Phoenix build without the press event: %+v", result)
	}

	result, err = scanAndroidAPK(writeAndroidCompatAPK(t, testAndroidDex()), []string{"accessibilityActions", "text", "missingProperty"})
	if err != nil {
		t.Fatal(err)
	}
	if result.Supported || len(result.MissingProperties) != 1 || result.MissingProperties[0] != "missingProperty" {
		t.Fatalf("scanner accepted a missing Phoenix property: %+v", result)
	}
}

func TestAndroidCompatCombinesBaseAndSplitAPKFiles(t *testing.T) {
	base := writeAndroidCompatAPK(t, testAndroidDex())
	split := writeAndroidAPK(t, filepath.Join(t.TempDir(), "config.arm64_v8a.apk"), "classes2.dex", testAndroidDex())
	resources := filepath.Join(t.TempDir(), "config.xxhdpi.apk")
	file, err := os.Create(resources)
	if err != nil {
		t.Fatal(err)
	}
	archive := zip.NewWriter(file)
	if _, err := archive.Create("resources.arsc"); err != nil {
		t.Fatal(err)
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	result, err := scanAndroidAPKs([]string{base, resources, split}, []string{"accessibilityActions"})
	if err != nil {
		t.Fatal(err)
	}
	if !result.Supported || len(result.APKs) != 2 || len(result.DexFiles) != 2 {
		t.Fatalf("split APK data was not combined: %+v", result)
	}
}

func stringsIndex(value []byte, target []byte) int {
	for index := 0; index+len(target) <= len(value); index++ {
		if string(value[index:index+len(target)]) == string(target) {
			return index
		}
	}
	return -1
}
