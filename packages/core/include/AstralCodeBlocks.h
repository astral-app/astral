#pragma once

#include "AstralRust.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint64_t message_id;
    uint64_t channel_id;
    uint32_t index;
    const char *code;
    size_t code_length;
} AstralCodeBlockContext;

typedef int (*AstralCodeBlockDecorator)(const AstralCodeBlockContext *context, AstralRustString *card);

void astral_wire_set_code_block_decorator(AstralCodeBlockDecorator decorator);
AstralRustString astral_code_highlight(const char *code, size_t codeLength, const char *language, size_t languageLength, const char *theme,
                                       size_t themeLength);

#ifdef __cplusplus
}
#endif
