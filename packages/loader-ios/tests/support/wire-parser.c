#include "wire-parser.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

enum { kMaxOps = 6000, kMaxDepth = 6, kMaxItems = 4096, kLineCap = 1 << 20 };

typedef struct {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
    int failed;
} Reader;

typedef struct {
    char *buffer;
    size_t length;
    int overflowed;
} Writer;

static int has(Reader *reader, size_t count)
{
    if (reader->failed || reader->length - reader->offset < count) {
        reader->failed = 1;
        return 0;
    }
    return 1;
}

static uint8_t u8(Reader *reader)
{
    return has(reader, 1) ? reader->bytes[reader->offset++] : 0;
}

static uint16_t u16(Reader *reader)
{
    if (!has(reader, 2)) return 0;
    uint16_t value = (uint16_t)(reader->bytes[reader->offset] | reader->bytes[reader->offset + 1] << 8);
    reader->offset += 2;
    return value;
}

static uint32_t u32(Reader *reader)
{
    if (!has(reader, 4)) return 0;
    uint32_t value;
    memcpy(&value, reader->bytes + reader->offset, 4);
    reader->offset += 4;
    return value;
}

static uint64_t u64(Reader *reader)
{
    if (!has(reader, 8)) return 0;
    uint64_t value;
    memcpy(&value, reader->bytes + reader->offset, 8);
    reader->offset += 8;
    return value;
}

static void emit(Writer *writer, const char *format, ...) __attribute__((format(printf, 2, 3)));

static void emit(Writer *writer, const char *format, ...)
{
    if (writer->overflowed) return;
    va_list arguments;
    va_start(arguments, format);
    int written = vsnprintf(writer->buffer + writer->length, kLineCap - writer->length, format, arguments);
    va_end(arguments);
    if (written < 0 || (size_t)written >= kLineCap - writer->length) {
        writer->overflowed = 1;
        return;
    }
    writer->length += (size_t)written;
}

static uint32_t fnv1a(const uint8_t *bytes, size_t length)
{
    uint32_t hash = 2166136261u;
    for (size_t index = 0; index < length; index++) {
        hash ^= bytes[index];
        hash *= 16777619u;
    }
    return hash;
}

static int identifier(const uint8_t *bytes, size_t length)
{
    if (!length || length > 48) return 0;
    for (size_t index = 0; index < length; index++) {
        uint8_t character = bytes[index];
        if (!((character >= 'a' && character <= 'z') || (character >= 'A' && character <= 'Z') ||
              (character >= '0' && character <= '9') || character == '_' || character == '-' ||
              character == '.' || character == ':' || character == '%' || character == '#')) return 0;
    }
    return 1;
}

static void text(Reader *reader, Writer *writer, uint32_t length, int keepIdentifier)
{
    if (!has(reader, length)) return;
    const uint8_t *bytes = reader->bytes + reader->offset;
    reader->offset += length;
    if (keepIdentifier && identifier(bytes, length)) {
        emit(writer, "\"%.*s\"", (int)length, bytes);
    } else {
        emit(writer, "{\"n\":%u,\"h\":\"%08x\"}", length, fnv1a(bytes, length));
    }
}

static void valueOfTag(Reader *reader, Writer *writer, uint8_t tag, int depth);

static int gKeepStrings;

static int textProperty(uint16_t property)
{
    return property == 499 || property == 11 || property == 354;
}

static void tagged(Reader *reader, Writer *writer, int depth)
{
    uint8_t tag = u8(reader);
    emit(writer, "[%u,", tag);
    valueOfTag(reader, writer, tag, depth);
    emit(writer, "]");
}

static void valueOfTag(Reader *reader, Writer *writer, uint8_t tag, int depth)
{
    if (depth > kMaxDepth) {
        reader->failed = 1;
        return;
    }
    switch (tag) {
    case 0: emit(writer, "null"); break;
    case 1: emit(writer, "false"); break;
    case 2: emit(writer, "true"); break;
    case 3: emit(writer, "%d", (int32_t)u32(reader)); break;
    case 4: emit(writer, "%lld", (long long)(int64_t)u64(reader)); break;
    case 5: {
        uint64_t raw = u64(reader);
        double number;
        memcpy(&number, &raw, 8);
        emit(writer, "%.6g", number);
        break;
    }
    case 6: text(reader, writer, u32(reader), gKeepStrings); break;
    case 7: {
        uint32_t count = u32(reader);
        if (count > kMaxItems) { reader->failed = 1; return; }
        emit(writer, "[");
        for (uint32_t index = 0; index < count && !reader->failed; index++) {
            if (index) emit(writer, ",");
            tagged(reader, writer, depth + 1);
        }
        emit(writer, "]");
        break;
    }
    case 8: {
        uint32_t count = u32(reader);
        if (count > kMaxItems) { reader->failed = 1; return; }
        emit(writer, "{\"m\":[");
        for (uint32_t index = 0; index < count && !reader->failed; index++) {
            if (index) emit(writer, ",");
            emit(writer, "[%u,", u16(reader));
            tagged(reader, writer, depth + 1);
            emit(writer, "]");
        }
        emit(writer, "]}");
        break;
    }
    case 9: {
        float quad[4];
        for (int index = 0; index < 4; index++) {
            uint32_t raw = u32(reader);
            memcpy(&quad[index], &raw, 4);
        }
        emit(writer, "[%.4g,%.4g,%.4g,%.4g]", quad[0], quad[1], quad[2], quad[3]);
        break;
    }
    case 10: {
        uint32_t count = u32(reader);
        if (count > kMaxItems) { reader->failed = 1; return; }
        emit(writer, "{\"k\":[");
        for (uint32_t index = 0; index < count && !reader->failed; index++) {
            if (index) emit(writer, ",");
            emit(writer, "[");
            text(reader, writer, u32(reader), 1);
            emit(writer, ",");
            tagged(reader, writer, depth + 1);
            emit(writer, "]");
        }
        emit(writer, "]}");
        break;
    }
    case 11: {
        uint32_t count = u32(reader);
        if (count > kMaxItems || !has(reader, (size_t)count * 4)) { reader->failed = 1; return; }
        reader->offset += (size_t)count * 4;
        emit(writer, "{\"words\":%u}", count);
        break;
    }
    case 12: {
        uint32_t cached = u32(reader);
        uint16_t edits = u16(reader);
        if (!has(reader, (size_t)edits * 6)) return;
        reader->offset += (size_t)edits * 6;
        emit(writer, "{\"cached\":%u,\"edits\":%u}", cached, edits);
        break;
    }
    default: reader->failed = 1; break;
    }
}

static void properties(Reader *reader, Writer *writer)
{
    uint16_t count = u16(reader);
    emit(writer, "[");
    for (uint16_t index = 0; index < count && !reader->failed; index++) {
        if (index) emit(writer, ",");
        uint16_t property = u16(reader);
        uint8_t tag = u8(reader);
        emit(writer, "[%u,%u,", property, tag);
        gKeepStrings = !textProperty(property);
        valueOfTag(reader, writer, tag, 0);
        gKeepStrings = 0;
        emit(writer, "]");
    }
    emit(writer, "]");
}

static void ids(Reader *reader, Writer *writer, uint32_t count)
{
    if (count > kMaxItems) { reader->failed = 1; return; }
    emit(writer, "[");
    for (uint32_t index = 0; index < count && !reader->failed; index++) {
        emit(writer, index ? ",%d" : "%d", (int32_t)u32(reader));
    }
    emit(writer, "]");
}

int astral_wire_summarise(const uint8_t *bytes, size_t length, AstralWireSink sink, void *context)
{
    static char buffer[kLineCap];
    Reader reader = {bytes, length, 0, 0};
    Writer writer = {buffer, 0, 0};
    uint8_t version = u8(&reader);
    emit(&writer, "{\"v\":%u,\"bytes\":%zu,\"ops\":[", version, length);
    int count = 0;
    if (version == 1) {
        while (!reader.failed && reader.offset < reader.length && count < kMaxOps) {
            uint8_t op = u8(&reader);
            emit(&writer, count ? ",[%u" : "[%u", op);
            switch (op) {
            case 1: {
                int32_t view = (int32_t)u32(&reader);
                uint16_t typeLength = u16(&reader);
                emit(&writer, ",%d,", view);
                text(&reader, &writer, typeLength, 1);
                emit(&writer, ",");
                properties(&reader, &writer);
                break;
            }
            case 2:
                emit(&writer, ",%d,", (int32_t)u32(&reader));
                properties(&reader, &writer);
                break;
            case 3: {
                emit(&writer, ",%d,", (int32_t)u32(&reader));
                ids(&reader, &writer, u32(&reader));
                break;
            }
            case 4:
            case 9:
                emit(&writer, ",");
                ids(&reader, &writer, u32(&reader));
                break;
            case 5: emit(&writer, ",%d", (int32_t)u32(&reader)); break;
            case 6:
                if (u8(&reader)) emit(&writer, ",%d", (int32_t)u32(&reader));
                break;
            case 7: {
                emit(&writer, ",%d,", (int32_t)u32(&reader));
                uint8_t tag = u8(&reader);
                emit(&writer, "[%u,", tag);
                valueOfTag(&reader, &writer, tag, 0);
                emit(&writer, "]");
                break;
            }
            case 8: break;
            case 10: {
                emit(&writer, ",%d,[", (int32_t)u32(&reader));
                uint32_t edits = u32(&reader);
                if (edits > kMaxItems) { reader.failed = 1; break; }
                for (uint32_t index = 0; index < edits && !reader.failed; index++) {
                    uint32_t keep = u32(&reader), drop = u32(&reader);
                    emit(&writer, index ? ",[%u,%u," : "[%u,%u,", keep, drop);
                    ids(&reader, &writer, u32(&reader));
                    emit(&writer, "]");
                }
                emit(&writer, "]");
                break;
            }
            default: reader.failed = 1; break;
            }
            emit(&writer, "]");
            count++;
        }
    }
    emit(&writer, "],\"complete\":%s,\"overflow\":%s}",
         (!reader.failed && reader.offset == reader.length) ? "true" : "false",
         writer.overflowed ? "true" : "false");
    if (writer.overflowed) {
        writer.length = (size_t)snprintf(buffer, kLineCap, "{\"v\":%u,\"bytes\":%zu,\"overflow\":true}", version, length);
    }
    sink(context, buffer, writer.length);
    return reader.failed ? -1 : count;
}
