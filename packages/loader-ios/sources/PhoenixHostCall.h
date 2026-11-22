#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef bool (*AstralHostCall)(void *context, const uint8_t *frame, size_t length, void *result);

typedef struct {
    uintptr_t textStart;
    uintptr_t textEnd;
    uintptr_t call;
} AstralHostCallImage;

size_t astral_host_call_swap(const AstralHostCallImage *image, AstralHostCall replacement, void **context);

#ifdef __cplusplus
}
#endif
