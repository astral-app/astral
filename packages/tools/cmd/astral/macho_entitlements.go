package main

import (
	"bytes"
	"debug/macho"
	"encoding/binary"
	"howett.net/plist"
)

const (
	machoLoadCodeSignature = 0x1d
	codeSignatureSuperBlob = 0xfade0cc0
	codeSignatureEntitled  = 0xfade7171
	codeSignatureSlotEnts  = 5
	superBlobHeaderSize    = 12
	blobIndexSize          = 8
	blobHeaderSize         = 8
	appGroupsEntitlement   = "com.apple.security.application-groups"
)

func codeSignatureBytes(data []byte) []byte {
	file, err := macho.NewFile(bytes.NewReader(data))
	if err != nil {
		return nil
	}
	for _, load := range file.Loads {
		raw := load.Raw()
		if len(raw) < 16 || binary.LittleEndian.Uint32(raw) != machoLoadCodeSignature {
			continue
		}
		offset := uint64(binary.LittleEndian.Uint32(raw[8:]))
		size := uint64(binary.LittleEndian.Uint32(raw[12:]))
		if offset+size > uint64(len(data)) {
			return nil
		}
		return data[offset : offset+size]
	}
	return nil
}

func entitlementsBlob(signature []byte, offset uint32) []byte {
	if uint64(offset)+blobHeaderSize > uint64(len(signature)) {
		return nil
	}
	blob := signature[offset:]
	if binary.BigEndian.Uint32(blob) != codeSignatureEntitled {
		return nil
	}
	length := binary.BigEndian.Uint32(blob[4:])
	if length < blobHeaderSize || uint64(length) > uint64(len(blob)) {
		return nil
	}
	return blob[blobHeaderSize:length]
}

func machoAppGroups(data []byte) []string {
	signature := codeSignatureBytes(data)
	if len(signature) < superBlobHeaderSize || binary.BigEndian.Uint32(signature) != codeSignatureSuperBlob {
		return nil
	}
	count := binary.BigEndian.Uint32(signature[8:])
	for index := uint32(0); index < count; index++ {
		entry := superBlobHeaderSize + uint64(index)*blobIndexSize
		if entry+blobIndexSize > uint64(len(signature)) {
			return nil
		}
		if binary.BigEndian.Uint32(signature[entry:]) != codeSignatureSlotEnts {
			continue
		}
		entitlements := entitlementsBlob(signature, binary.BigEndian.Uint32(signature[entry+4:]))
		if entitlements == nil {
			return nil
		}
		var values map[string]interface{}
		if _, err := plist.Unmarshal(entitlements, &values); err != nil {
			return nil
		}
		raw, ok := values[appGroupsEntitlement].([]interface{})
		if !ok {
			return nil
		}
		groups := make([]string, 0, len(raw))
		for _, value := range raw {
			if group, ok := value.(string); ok && group != "" {
				groups = append(groups, group)
			}
		}
		return groups
	}
	return nil
}
