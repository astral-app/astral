#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    const uint8_t *image;
    size_t length;
    int64_t slide;
    int file;
} AstralImageView;

uint64_t astral_locate_function_referencing(const AstralImageView *view, const char *needle);
uint64_t astral_locate_unique_caller(const AstralImageView *view, uint64_t callee);
uint64_t astral_locate_host_call(const AstralImageView *view, uint64_t opsArrived);
size_t astral_read_property_names(const AstralImageView *view, const char **names, size_t capacity, char *storage, size_t storageSize);

#ifdef __cplusplus
}
#endif
