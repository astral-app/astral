#include "PhoenixLocate.h"

#include <mach-o/loader.h>
#include <string.h>

enum { kMaxSegments = 16, kMaxTargets = 8 };

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

uint64_t astral_locate_function_referencing(const AstralImageView *view, const char *needle)
{
    Image image = {view, {{0}}, 0, 0, 0, 0, 0};
    const struct mach_header_64 *header = (const struct mach_header_64 *)view->image;
    if (!header || header->magic != MH_MAGIC_64) return 0;
    const struct section_64 *text = NULL;
    const struct section_64 *strings[4] = {0};
    size_t stringCount = 0;
    const struct linkedit_data_command *starts = NULL;
    const uint8_t *cursor = (const uint8_t *)(header + 1);
    for (uint32_t index = 0; index < header->ncmds; index++) {
        const struct load_command *command = (const struct load_command *)cursor;
        if (command->cmd == LC_SEGMENT_64) {
            const struct segment_command_64 *segment = (const struct segment_command_64 *)command;
            if (image.segmentCount < kMaxSegments)
                image.segments[image.segmentCount++] = (Segment){segment->vmaddr, segment->vmsize, segment->fileoff};
            if (strcmp(segment->segname, "__TEXT") == 0) image.textBase = segment->vmaddr;
            if (strcmp(segment->segname, "__LINKEDIT") == 0) {
                image.linkeditVm = segment->vmaddr;
                image.linkeditFile = segment->fileoff;
            }
            const struct section_64 *section = (const struct section_64 *)(segment + 1);
            for (uint32_t sectionIndex = 0; sectionIndex < segment->nsects; sectionIndex++, section++) {
                if (strcmp(section->segname, "__TEXT") != 0) continue;
                if (strcmp(section->sectname, "__text") == 0)
                    text = section;
                else if ((strcmp(section->sectname, "__oslogstring") == 0 || strcmp(section->sectname, "__cstring") == 0) &&
                         stringCount < 4)
                    strings[stringCount++] = section;
            }
        } else if (command->cmd == LC_FUNCTION_STARTS) {
            starts = (const struct linkedit_data_command *)command;
        }
        cursor += command->cmdsize;
    }
    if (!text || !starts) return 0;
    image.slide = (int64_t)(uintptr_t)view->image - (int64_t)image.textBase;
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
