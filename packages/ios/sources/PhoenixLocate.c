#include "PhoenixLocate.h"

#include <mach-o/loader.h>
#include <string.h>

enum { kMaxSegments = 16, kMaxTargets = 8, kMaxDataSections = 32 };

typedef struct {
    uint64_t vmaddr;
    uint64_t vmsize;
    uint64_t fileoff;
} Segment;

typedef struct {
    const AstralImageView *view;
    Segment segments[kMaxSegments];
    size_t segmentCount;
    uint64_t textBase;
    uint64_t linkeditVm;
    uint64_t linkeditFile;
    int64_t slide;
} Image;

static const uint8_t *pointerAt(const Image *image, uint64_t vmaddr, size_t length)
{
    if (!image->view->file) return (const uint8_t *)(uintptr_t)(vmaddr + (uint64_t)image->slide);
    for (size_t index = 0; index < image->segmentCount; index++) {
        const Segment *segment = &image->segments[index];
        if (vmaddr >= segment->vmaddr && vmaddr + length <= segment->vmaddr + segment->vmsize) {
            uint64_t offset = segment->fileoff + (vmaddr - segment->vmaddr);
            return offset + length <= image->view->length ? image->view->image + offset : NULL;
        }
    }
    return NULL;
}

static uint64_t findString(const Image *image, const struct section_64 *section, const char *needle)
{
    size_t size = strlen(needle);
    const uint8_t *bytes = pointerAt(image, section->addr, section->size);
    if (!bytes || section->size < size) return 0;
    for (uint64_t offset = 0; offset + size <= section->size; offset++) {
        if (bytes[offset] != (uint8_t)needle[0] || memcmp(bytes + offset, needle, size) != 0) continue;
        uint64_t start = offset;
        while (start > 0 && bytes[start - 1] != 0) start--;
        return section->addr + start;
    }
    return 0;
}

static uint64_t functionStart(const Image *image, const struct linkedit_data_command *starts, uint64_t address)
{
    const uint8_t *bytes = image->view->file
                               ? (starts->dataoff + starts->datasize <= image->view->length ? image->view->image + starts->dataoff : NULL)
                               : pointerAt(image, image->linkeditVm + (starts->dataoff - image->linkeditFile), starts->datasize);
    if (!bytes) return 0;
    uint64_t current = image->textBase, best = 0;
    for (uint32_t index = 0; index < starts->datasize;) {
        uint64_t value = 0;
        int shift = 0;
        uint8_t byte;
        do {
            byte = bytes[index++];
            value |= (uint64_t)(byte & 0x7f) << shift;
            shift += 7;
        } while ((byte & 0x80) && index < starts->datasize);
        if (!value) break;
        current += value;
        if (current > address) break;
        best = current;
    }
    return best;
}

typedef struct {
    Image image;
    const struct section_64 *text;
    const struct section_64 *strings[4];
    size_t stringCount;
    const struct linkedit_data_command *starts;
    const struct section_64 *data[kMaxDataSections];
    size_t dataCount;
} Parsed;

static int parse(const AstralImageView *view, Parsed *parsed)
{
    memset(parsed, 0, sizeof(*parsed));
    parsed->image.view = view;
    Image *image = &parsed->image;
    const struct mach_header_64 *header = (const struct mach_header_64 *)view->image;
    if (!header || header->magic != MH_MAGIC_64) return 0;
    const uint8_t *cursor = (const uint8_t *)(header + 1);
    for (uint32_t index = 0; index < header->ncmds; index++) {
        const struct load_command *command = (const struct load_command *)cursor;
        if (command->cmd == LC_SEGMENT_64) {
            const struct segment_command_64 *segment = (const struct segment_command_64 *)command;
            if (image->segmentCount < kMaxSegments)
                image->segments[image->segmentCount++] = (Segment){segment->vmaddr, segment->vmsize, segment->fileoff};
            if (strcmp(segment->segname, "__TEXT") == 0) image->textBase = segment->vmaddr;
            if (strcmp(segment->segname, "__LINKEDIT") == 0) {
                image->linkeditVm = segment->vmaddr;
                image->linkeditFile = segment->fileoff;
            }
            const struct section_64 *section = (const struct section_64 *)(segment + 1);
            for (uint32_t sectionIndex = 0; sectionIndex < segment->nsects; sectionIndex++, section++) {
                if (strncmp(section->segname, "__DATA", 6) == 0 && (section->flags & SECTION_TYPE) != S_ZEROFILL &&
                    parsed->dataCount < kMaxDataSections)
                    parsed->data[parsed->dataCount++] = section;
                if (strcmp(section->segname, "__TEXT") != 0) continue;
                if (strcmp(section->sectname, "__text") == 0)
                    parsed->text = section;
                else if ((strcmp(section->sectname, "__oslogstring") == 0 || strcmp(section->sectname, "__cstring") == 0) &&
                         parsed->stringCount < 4)
                    parsed->strings[parsed->stringCount++] = section;
            }
        } else if (command->cmd == LC_FUNCTION_STARTS) {
            parsed->starts = (const struct linkedit_data_command *)command;
        }
        cursor += command->cmdsize;
    }
    if (!parsed->text || !parsed->starts) return 0;
    image->slide = (int64_t)(uintptr_t)view->image - (int64_t)image->textBase;
    return 1;
}

uint64_t astral_locate_unique_caller(const AstralImageView *view, uint64_t callee)
{
    Parsed parsed;
    if (!callee || !parse(view, &parsed)) return 0;
    const struct section_64 *text = parsed.text;
    const uint32_t *words = (const uint32_t *)pointerAt(&parsed.image, text->addr, text->size);
    if (!words) return 0;
    uint64_t target = parsed.image.textBase + callee, found = 0;
    size_t count = text->size / 4;
    for (size_t index = 0; index < count; index++) {
        uint32_t word = words[index];
        if ((word & 0xfc000000) != 0x94000000) continue;
        int64_t immediate = (int64_t)(word & 0x3ffffff);
        if (immediate & (1 << 25)) immediate -= 1 << 26;
        uint64_t pc = text->addr + index * 4;
        if (pc + (uint64_t)(immediate * 4) != target) continue;
        uint64_t function = functionStart(&parsed.image, parsed.starts, pc);
        if (!function || (found && found != function)) return 0;
        found = function;
    }
    return found ? found - parsed.image.textBase : 0;
}

uint64_t astral_locate_host_call(const AstralImageView *view, uint64_t opsArrived)
{
    uint64_t dispatcher = astral_locate_unique_caller(view, opsArrived);
    uint64_t frame = astral_locate_unique_caller(view, dispatcher);
    return astral_locate_unique_caller(view, frame);
}

uint64_t astral_locate_function_referencing(const AstralImageView *view, const char *needle)
{
    Parsed parsed;
    if (!parse(view, &parsed)) return 0;
    Image image = parsed.image;
    const struct section_64 *text = parsed.text;
    const struct section_64 *const *strings = parsed.strings;
    size_t stringCount = parsed.stringCount;
    const struct linkedit_data_command *starts = parsed.starts;
    uint64_t targets[kMaxTargets];
    size_t targetCount = 0;
    for (size_t index = 0; index < stringCount && targetCount + 2 <= kMaxTargets; index++) {
        uint64_t address = findString(&image, strings[index], needle);
        if (!address) continue;
        targets[targetCount++] = address;
        targets[targetCount++] = address - 0x20;
    }
    if (!targetCount) return 0;
    const uint32_t *words = (const uint32_t *)pointerAt(&image, text->addr, text->size);
    if (!words) return 0;
    uint64_t found = 0;
    size_t count = text->size / 4;
    for (size_t index = 0; index < count; index++) {
        uint32_t word = words[index];
        if ((word & 0x9f000000) != 0x90000000) continue;
        uint32_t rd = word & 31;
        int64_t immediate = (int64_t)((((word >> 5) & 0x7ffff) << 2) | ((word >> 29) & 3));
        if (immediate & (1 << 20)) immediate -= 1 << 21;
        uint64_t pc = text->addr + index * 4;
        uint64_t page = (pc & ~(uint64_t)0xfff) + (uint64_t)(immediate << 12);
        for (size_t next = 1; next <= 4 && index + next < count; next++) {
            uint32_t add = words[index + next];
            if ((add & 0xffc00000) != 0x91000000 || ((add >> 5) & 31) != rd) continue;
            uint64_t target = page + ((add >> 10) & 0xfff);
            for (size_t candidate = 0; candidate < targetCount; candidate++) {
                if (target != targets[candidate]) continue;
                uint64_t function = functionStart(&image, starts, pc);
                if (!function) return 0;
                if (found && found != function) return 0;
                found = function;
            }
            break;
        }
    }
    return found ? found - image.textBase : 0;
}

static const uint8_t kPropertyAnchor[16] = {'t', 'e', 'x', 't', 'T', 'r', 'a', 'n', 's', 'f', 'o', 'r', 'm', 0, 0, 0xED};

static int printable(const uint8_t *bytes, size_t length)
{
    for (size_t index = 0; index < length; index++) {
        if (bytes[index] <= 32 || bytes[index] >= 127) return 0;
    }
    return length > 0;
}

static size_t decodeString(const Parsed *parsed, const uint8_t *entry, const uint8_t **text)
{
    uint8_t discriminator = entry[15];
    if ((discriminator >> 4) == 0xE) {
        size_t length = discriminator & 0xF;
        for (size_t index = length; index < 15; index++) {
            if (entry[index]) return 0;
        }
        *text = entry;
        return printable(entry, length) ? length : 0;
    }
    uint64_t flags, object;
    memcpy(&flags, entry, 8);
    memcpy(&object, entry + 8, 8);
    uint8_t kind = (uint8_t)(flags >> 60);
    if (kind != 0x8 && kind != 0x9 && kind != 0xC && kind != 0xD) return 0;
    uint64_t length = flags & 0xFFFFFFFFFFFFull;
    if (length == 0 || length > 255) return 0;
    uint64_t address;
    if (parsed->image.view->file) {
        if (((object >> 36) & 0xFF) != 0x80) return 0;
        address = parsed->image.textBase + (object & 0xFFFFFFFFFull) + 32;
    } else {
        if ((object >> 60) != 0x8) return 0;
        address = (object & 0x0FFFFFFFFFFFFFFFull) + 32 - (uint64_t)parsed->image.slide;
    }
    const uint8_t *bytes = pointerAt(&parsed->image, address, (size_t)length);
    if (!bytes || !printable(bytes, (size_t)length)) return 0;
    *text = bytes;
    return (size_t)length;
}

size_t astral_read_property_names(const AstralImageView *view, const char **names, size_t capacity, char *storage, size_t storageSize)
{
    Parsed parsed;
    if (!parse(view, &parsed)) return 0;
    for (size_t sectionIndex = 0; sectionIndex < parsed.dataCount; sectionIndex++) {
        const struct section_64 *section = parsed.data[sectionIndex];
        const uint8_t *bytes = pointerAt(&parsed.image, section->addr, section->size);
        if (!bytes || section->size < 16) continue;
        for (uint64_t offset = 0; offset + 16 <= section->size; offset += 8) {
            if (memcmp(bytes + offset, kPropertyAnchor, 16) != 0) continue;
            const uint8_t *text;
            uint64_t start = offset;
            while (start >= 16 && decodeString(&parsed, bytes + start - 16, &text)) start -= 16;
            size_t count = 0, used = 0;
            for (uint64_t at = start; at + 16 <= section->size && count < capacity; at += 16) {
                size_t length = decodeString(&parsed, bytes + at, &text);
                if (!length || used + length + 1 > storageSize) break;
                memcpy(storage + used, text, length);
                storage[used + length] = 0;
                names[count++] = storage + used;
                used += length + 1;
            }
            return count;
        }
    }
    return 0;
}
