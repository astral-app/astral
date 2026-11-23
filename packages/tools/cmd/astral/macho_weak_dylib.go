package main

import (
	"bytes"
	"debug/macho"
	"encoding/binary"
	"errors"
	"fmt"
	"os"
)

const (
	weakDylibLoadCommand = 0x80000018
	dylibIDLoadCommand   = 0x0000000d
	machOHeaderSize64    = 32
	dylibCommandSize     = 24
)

func alignedDylibCommand(name string) []byte {
	size := (dylibCommandSize + len(name) + 1 + 7) &^ 7
	command := make([]byte, size)
	binary.LittleEndian.PutUint32(command[0:], weakDylibLoadCommand)
	binary.LittleEndian.PutUint32(command[4:], uint32(size))
	binary.LittleEndian.PutUint32(command[8:], dylibCommandSize)
	binary.LittleEndian.PutUint32(command[12:], 2)
	binary.LittleEndian.PutUint32(command[16:], 0x10000)
	binary.LittleEndian.PutUint32(command[20:], 0x10000)
	copy(command[dylibCommandSize:], name)
	return command
}

func firstSectionOffset(file *macho.File) uint32 {
	first := ^uint32(0)
	for _, section := range file.Sections {
		if section.Offset != 0 && section.Offset < first {
			first = section.Offset
		}
	}
	return first
}

func loadedDylibName(raw []byte) (string, bool) {
	if len(raw) < dylibCommandSize {
		return "", false
	}
	switch binary.LittleEndian.Uint32(raw) {
	case uint32(macho.LoadCmdDylib), weakDylibLoadCommand, dylibIDLoadCommand:
	default:
		return "", false
	}
	offset := binary.LittleEndian.Uint32(raw[8:])
	if offset >= uint32(len(raw)) {
		return "", false
	}
	name := raw[offset:]
	if end := bytes.IndexByte(name, 0); end >= 0 {
		name = name[:end]
	}
	return string(name), true
}

func machoLoadsDylib(file *macho.File, name string) bool {
	for _, load := range file.Loads {
		if loaded, ok := loadedDylibName(load.Raw()); ok && loaded == name {
			return true
		}
	}
	return false
}

func addWeakDylibBytes(data []byte, name string) ([]byte, error) {
	file, err := macho.NewFile(bytes.NewReader(data))
	if err != nil {
		return nil, fmt.Errorf("binary is not a thin Mach-O: %w", err)
	}
	if file.Magic != macho.Magic64 || file.ByteOrder != binary.LittleEndian {
		return nil, errors.New("binary is not a little-endian 64-bit Mach-O")
	}
	if machoLoadsDylib(file, name) {
		return data, nil
	}
	command := alignedDylibCommand(name)
	end := uint64(machOHeaderSize64) + uint64(file.Cmdsz)
	commandEnd := end + uint64(len(command))
	if commandEnd > uint64(firstSectionOffset(file)) || commandEnd > uint64(len(data)) {
		return nil, errors.New("binary has no room for another load command")
	}
	for _, value := range data[end:commandEnd] {
		if value != 0 {
			return nil, errors.New("binary has data after its load commands")
		}
	}
	patched := append([]byte(nil), data...)
	copy(patched[end:], command)
	binary.LittleEndian.PutUint32(patched[16:], file.Ncmd+1)
	binary.LittleEndian.PutUint32(patched[20:], file.Cmdsz+uint32(len(command)))
	return patched, nil
}

func addWeakDylib(path string, name string) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	patched, err := addWeakDylibBytes(data, name)
	if err != nil {
		return err
	}
	return os.WriteFile(path, patched, 0o755)
}
