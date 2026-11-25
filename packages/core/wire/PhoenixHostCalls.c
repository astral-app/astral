#include "PhoenixWireInternal.h"

#include <stdlib.h>
#include <string.h>

enum { kCallHeader = 16, kCallKind = 1, kMaxCallUri = 1024 };

static AstralImageRewriter gCallRewriter;
static unsigned gRewrittenCalls;

void astral_host_calls_set_image_rewriter(AstralImageRewriter rewriter)
{
    gCallRewriter = rewriter;
}

unsigned astral_host_calls_rewritten(void)
{
    return gRewrittenCalls;
}

static int copyValue(Cursor *cursor, Out *out, int depth, int *changed)
{
    if (depth > 8 || cursor->failed) return 0;
    size_t start = cursor->offset;
    uint8_t tag = wireRead8(cursor);
    if (tag == 6) {
        uint32_t length = wireRead32(cursor);
        if (!wireNeed(cursor, length)) return 0;
        const char *text = (const char *)cursor->bytes + cursor->offset;
        char uri[kMaxCallUri], rewritten[kMaxCallUri];
        if (gCallRewriter && length > 8 && length < kMaxCallUri && memcmp(text, "https://", 8) == 0) {
            memcpy(uri, text, length);
            uri[length] = 0;
            if (gCallRewriter(uri, rewritten, sizeof rewritten) && strcmp(uri, rewritten) != 0) {
                size_t size = strlen(rewritten);
                wirePut8(out, 6);
                wirePut32(out, (uint32_t)size);
                wirePut(out, rewritten, size);
                cursor->offset += length;
                *changed = 1;
                return 1;
            }
        }
        cursor->offset += length;
        wirePut(out, cursor->bytes + start, cursor->offset - start);
        return 1;
    }
    if (tag == 7 || tag == 8 || tag == 10) {
        uint32_t count = wireRead32(cursor);
        wirePut8(out, tag);
        wirePut32(out, count);
        for (uint32_t index = 0; index < count && !cursor->failed; index++) {
            if (tag == 8) {
                wirePut16(out, wireRead16(cursor));
            } else if (tag == 10) {
                uint32_t keyLength = wireRead32(cursor);
                if (!wireNeed(cursor, keyLength)) return 0;
                wirePut32(out, keyLength);
                wirePut(out, cursor->bytes + cursor->offset, keyLength);
                cursor->offset += keyLength;
            }
            if (!copyValue(cursor, out, depth + 1, changed)) return 0;
        }
        return !cursor->failed;
    }
    wireSkipValue(cursor, tag, depth);
    if (cursor->failed) return 0;
    wirePut(out, cursor->bytes + start, cursor->offset - start);
    return 1;
}

int astral_host_call_rewrite(const uint8_t *frame, size_t length, AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    if (!gCallRewriter || length < kCallHeader || frame[0] != kCallKind) return 0;
    uint32_t argumentsLength;
    memcpy(&argumentsLength, frame + 12, 4);
    if (argumentsLength == 0 || argumentsLength > length - kCallHeader) return 0;
    Cursor cursor = {frame, kCallHeader + argumentsLength, kCallHeader, 0};
    Out arguments = {0};
    int changed = 0;
    while (cursor.offset < cursor.length && !cursor.failed)
        if (!copyValue(&cursor, &arguments, 0, &changed)) break;
    if (!changed || cursor.failed || cursor.offset != cursor.length || arguments.failed) {
        free(arguments.bytes);
        return 0;
    }
    Out out = {0};
    wirePut(&out, frame, 12);
    wirePut32(&out, (uint32_t)arguments.length);
    wirePut(&out, arguments.bytes, arguments.length);
    wirePut(&out, frame + kCallHeader + argumentsLength, length - kCallHeader - argumentsLength);
    free(arguments.bytes);
    if (out.failed) {
        free(out.bytes);
        return 0;
    }
    gRewrittenCalls++;
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
