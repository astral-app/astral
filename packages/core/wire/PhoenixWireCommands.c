#include "PhoenixWireInternal.h"

#include <pthread.h>
#include <stdlib.h>
#include <string.h>

static pthread_mutex_t gLock = PTHREAD_MUTEX_INITIALIZER;
static Out gPending;

static void putStringValue(Out *out, uint16_t key, const char *value)
{
    wirePut16(out, key);
    wirePut8(out, 6);
    wirePut32(out, (uint32_t)strlen(value));
    wirePut(out, value, strlen(value));
}

static void queue(const Out *command)
{
    pthread_mutex_lock(&gLock);
    wirePut(&gPending, command->bytes, command->length);
    pthread_mutex_unlock(&gLock);
}

static const char *alignment(const char *align)
{
    if (align && (strcmp(align, "start") == 0 || strcmp(align, "end") == 0)) return align;
    return "visible";
}

int astral_wire_scroll_to_message(uint64_t message, int animated, const char *align)
{
    int32_t list = 0;
    int64_t index = 0;
    if (!message || !wireRowOfMessage(message, &list, &index) || index < 0 || index > INT32_MAX) return 0;
    Out command = {0};
    wirePut8(&command, 7);
    wirePut32(&command, (uint32_t)list);
    wirePut8(&command, 8);
    wirePut32(&command, 4);
    wirePut16(&command, P(index));
    wirePut8(&command, 3);
    wirePut32(&command, (uint32_t)index);
    putStringValue(&command, P(align), alignment(align));
    wirePut16(&command, P(offset));
    wirePut8(&command, 5);
    double offset = 0;
    wirePut(&command, &offset, sizeof(offset));
    wirePut16(&command, P(animated));
    wirePut8(&command, animated ? 2 : 1);
    if (!command.failed) queue(&command);
    free(command.bytes);
    return !command.failed;
}

int astral_wire_focus_composer(int focus)
{
    int32_t composer = wireComposerNode();
    if (focus && !composer) return 0;
    Out command = {0};
    wirePut8(&command, 6);
    wirePut8(&command, focus ? 1 : 0);
    if (focus) wirePut32(&command, (uint32_t)composer);
    if (!command.failed) queue(&command);
    free(command.bytes);
    return !command.failed;
}

int astral_wire_take_commands(AstralWireBuffer *buffer)
{
    memset(buffer, 0, sizeof(*buffer));
    pthread_mutex_lock(&gLock);
    Out pending = gPending;
    memset(&gPending, 0, sizeof(gPending));
    pthread_mutex_unlock(&gLock);
    if (!pending.length || pending.failed) {
        free(pending.bytes);
        return 0;
    }
    Out out = {0};
    wirePut8(&out, kWireVersion);
    wirePut(&out, pending.bytes, pending.length);
    free(pending.bytes);
    if (out.failed) {
        free(out.bytes);
        return 0;
    }
    buffer->bytes = out.bytes;
    buffer->length = out.length;
    return 1;
}

uint64_t astral_wire_message_author(uint64_t message)
{
    return wireMessageAuthor(message);
}
