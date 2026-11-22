#include "PhoenixWireInternal.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int gUnsupportedWireVersion;
static unsigned gRewriteFailures;

unsigned astral_wire_rewrite_failures(void)
{
    return gRewriteFailures;
}

int astral_wire_unsupported_version(void)
{
    return gUnsupportedWireVersion;
}

int astral_settings_rewrite(const uint8_t *bytes, size_t length, AstralWireBuffer *buffer)
{
    static Span spans[kMaxSpans];
    size_t count = 0;
    memset(buffer, 0, sizeof(*buffer));
    if (length && bytes[0] != kWireVersion && !gUnsupportedWireVersion) gUnsupportedWireVersion = bytes[0];
    if (gUnsupportedWireVersion) return 0;
    if (wireScan(bytes, length, spans, &count) != 0) return 0;

    Out out = {0};
    int changed = 0;
    size_t relabelledLength = 0;
    uint8_t *relabelled = wireRelabel(bytes, length, spans, count, &relabelledLength);
    if (relabelled) {
        bytes = relabelled;
        length = relabelledLength;
        changed = 1;
    }
    wireTrackColors(bytes, spans, count);
    const uint8_t *raw = bytes;
    uint8_t *themed = NULL;
    if (wireThemeActive()) {
        themed = malloc(length);
        if (themed) {
            memcpy(themed, bytes, length);
            if (wirePatchColors(themed, spans, count) || changed) {
                bytes = themed;
                changed = 1;
            }
        }
    }
    if (changed) wirePut(&out, bytes, length);
    wireObserve(bytes, spans, count);
    wireSetTemplateSource(raw);
    wireRewritePages(bytes, spans, count, &out, &changed, length);
    wireSetTemplateSource(NULL);
    wireDecorate(bytes, spans, count, &out, &changed, length);
    wireDecorateMentions(bytes, spans, count, &out, &changed, length);
    wireSettingsBackdrop(bytes, spans, count, &out, &changed, length);
    wireObserveRemovals(bytes, spans, count, &out, &changed, length);
    free(themed);
    free(relabelled);
    if (!changed || out.failed) {
        free(out.bytes);
        return 0;
    }
    size_t checked = 0;
    if (wireScan(out.bytes, out.length, spans, &checked) != 0) {
        gRewriteFailures++;
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

void astral_wire_buffer_free(AstralWireBuffer *buffer)
{
    free(buffer->bytes);
    buffer->bytes = NULL;
    buffer->length = 0;
}

int astral_wire_theme_page(AstralWireBuffer *buffer)
{
    static Span spans[kMaxSpans];
    size_t count = 0;
    if (!buffer->bytes || wireScan(buffer->bytes, buffer->length, spans, &count) != 0) return 0;
    wireTrackColors(buffer->bytes, spans, count);
    int changed = wireThemeActive() && wirePatchColors(buffer->bytes, spans, count);
    Out out = {0};
    int appended = 0;
    wireSettingsBackdrop(buffer->bytes, spans, count, &out, &appended, buffer->length);
    if (!appended || out.failed) {
        free(out.bytes);
        return changed;
    }
    size_t checked = 0;
    if (wireScan(out.bytes, out.length, spans, &checked) != 0) {
        free(out.bytes);
        return changed;
    }
    free(buffer->bytes);
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}
