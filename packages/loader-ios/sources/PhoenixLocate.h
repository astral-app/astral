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

#ifdef __cplusplus
}
#endif
