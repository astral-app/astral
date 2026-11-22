#pragma once
#import <Foundation/Foundation.h>
#import <mach-o/dyld.h>
#import <mach-o/loader.h>
#import <uuid/uuid.h>
#import <string.h>

static inline NSString *AstralImageUUID(const struct mach_header *header) {
    if (!header || header->magic != MH_MAGIC_64) return @"";
    const struct mach_header_64 *image = (const void *)header;
    const uint8_t *cursor = (const void *)(image + 1);
    size_t remaining = image->sizeofcmds;
    for (uint32_t i = 0; i < image->ncmds; i++) {
        if (remaining < sizeof(struct load_command)) break;
        const struct load_command *command = (const void *)cursor;
        if (command->cmdsize < sizeof(*command) || command->cmdsize > remaining) break;
        if (command->cmd == LC_UUID && command->cmdsize >= sizeof(struct uuid_command)) {
            const struct uuid_command *uuid = (const void *)command;
            uuid_string_t value;
            uuid_unparse_lower(uuid->uuid, value);
            return [NSString stringWithUTF8String:value];
        }
        cursor += command->cmdsize;
        remaining -= command->cmdsize;
    }
    return @"";
}

static inline NSString *AstralImageUUIDForPath(const char *path) {
    if (!path) return @"";
    for (uint32_t i = 0; i < _dyld_image_count(); i++) {
        const char *candidate = _dyld_get_image_name(i);
        if (candidate && strcmp(candidate, path) == 0)
            return AstralImageUUID(_dyld_get_image_header(i));
    }
    return @"";
}
