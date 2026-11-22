#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include "PhoenixWireInternal.h"
#include "../support/batch-builder.h"
#include "../support/rewriter-setup.h"

static double now(void)
{
    struct timespec time;
    clock_gettime(CLOCK_MONOTONIC, &time);
    return time.tv_sec + time.tv_nsec / 1e9;
}

static void measure(const char *name, const unsigned char *bytes, size_t length, int rounds)
{
    double started = now();
    for (int round = 0; round < rounds; round++) {
        AstralWireBuffer out;
        if (astral_settings_rewrite(bytes, length, &out)) astral_wire_buffer_free(&out);
    }
    double perBatch = (now() - started) / rounds;
    printf("{\"batch\":\"%s\",\"bytes\":%zu,\"microseconds\":%.2f,\"megabytesPerSecond\":%.1f}\n", name, length, perBatch * 1e6,
           length / perBatch / 1e6);
}

static unsigned char *readFile(const char *path, size_t *length)
{
    FILE *file = fopen(path, "rb");
    if (!file) return NULL;
    fseek(file, 0, SEEK_END);
    long size = ftell(file);
    fseek(file, 0, SEEK_SET);
    unsigned char *bytes = size > 0 ? malloc((size_t)size) : NULL;
    if (bytes && fread(bytes, 1, (size_t)size, file) != (size_t)size) {
        free(bytes);
        bytes = NULL;
    }
    fclose(file);
    *length = bytes ? (size_t)size : 0;
    return bytes;
}

int main(int count, char **arguments)
{
    int rounds = 2000;
    static Batch batch;
    overview(&batch);
    measure("overview", batch.bytes, batch.length, rounds);
    setupRewriter();
    overview(&batch);
    measure("overview-themed", batch.bytes, batch.length, rounds);
    chat(&batch, 300);
    measure("chat-300-rows", batch.bytes, batch.length, 500);
    for (int index = 1; index < count; index++) {
        size_t length;
        unsigned char *bytes = readFile(arguments[index], &length);
        if (!bytes) continue;
        measure(arguments[index], bytes, length, 200);
        free(bytes);
    }
    return 0;
}
