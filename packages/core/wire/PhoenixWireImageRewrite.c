#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

enum { kImageUriLength = 1024 };

static AstralImageRewriter gRewriter;

void astral_wire_set_image_rewriter(AstralImageRewriter rewriter)
{
    gRewriter = rewriter;
}

static int readUri(const uint8_t *bytes, const Span *span, char *uri)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    uint16_t count = wireRead16(&cursor);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        if (property == P(uri) && tag == 6) {
            uint32_t length = wireRead32(&cursor);
            if (!wireNeed(&cursor, length) || length >= kImageUriLength) return 0;
            memcpy(uri, bytes + cursor.offset, length);
            uri[length] = 0;
            return 1;
        }
        wireSkipValue(&cursor, tag, 0);
    }
    return 0;
}

static void putRewrittenSpan(Out *out, const uint8_t *bytes, const Span *span, const char *uri)
{
    Cursor cursor = {bytes, span->end, span->start + 1 + 4, 0};
    size_t header = cursor.offset;
    if (span->op == 1) wireSkip(&cursor, wireRead16(&cursor));
    wirePut(out, bytes + span->start, cursor.offset - span->start);
    uint16_t count = wireRead16(&cursor);
    wirePut16(out, count);
    for (uint16_t index = 0; index < count && !cursor.failed; index++) {
        size_t start = cursor.offset;
        uint16_t property = wireRead16(&cursor);
        uint8_t tag = wireRead8(&cursor);
        wireSkipValue(&cursor, tag, 0);
        if ((property == P(uri) || property == P(cacheKey)) && tag == 6)
            wirePropString(out, property, uri);
        else
            wirePut(out, bytes + start, cursor.offset - start);
    }
    (void)header;
}

uint8_t *wireRewriteImages(const uint8_t *bytes, size_t length, Span *spans, size_t *count, size_t *newLength)
{
    if (!gRewriter) return NULL;
    Out out = {0};
    size_t copied = 0;
    int rewrote = 0;
    char uri[kImageUriLength], rewritten[kImageUriLength];
    for (size_t index = 0; index < *count; index++) {
        const Span *span = &spans[index];
        if (span->op != 1 && span->op != 2) continue;
        if (span->op == 1 && !wireTypeIs(bytes, span, "image")) continue;
        if (!readUri(bytes, span, uri) || strncmp(uri, "http", 4) != 0) continue;
        if (!gRewriter(uri, rewritten, sizeof rewritten) || strcmp(uri, rewritten) == 0) continue;
        wirePut(&out, bytes + copied, span->start - copied);
        putRewrittenSpan(&out, bytes, span, rewritten);
        copied = span->end;
        rewrote = 1;
    }
    if (!rewrote) return NULL;
    wirePut(&out, bytes + copied, length - copied);
    if (out.failed || wireScan(out.bytes, out.length, spans, count) != 0) {
        free(out.bytes);
        wireScan(bytes, length, spans, count);
        return NULL;
    }
    *newLength = out.length;
    return out.bytes;
}
