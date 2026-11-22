#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "monocypher-ed25519.h"

static size_t readAll(const char *path, unsigned char *buffer, size_t capacity)
{
    FILE *file = fopen(path, "rb");
    if (!file) return 0;
    size_t length = fread(buffer, 1, capacity, file);
    fclose(file);
    return length;
}

static int decode(const char *text, unsigned char *out, size_t capacity)
{
    static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    unsigned int value = 0;
    int bits = 0;
    size_t length = 0;
    for (const char *cursor = text; *cursor && *cursor != '='; cursor++) {
        const char *found = strchr(alphabet, *cursor);
        if (!found || !*cursor) continue;
        value = (value << 6) | (unsigned int)(found - alphabet);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            if (length >= capacity) return -1;
            out[length++] = (unsigned char)((value >> bits) & 0xFF);
        }
    }
    return (int)length;
}

int main(int count, char **arguments)
{
    if (count != 4) return 2;
    static unsigned char message[1 << 20], text[256], key[32], signature[64];
    size_t messageLength = readAll(arguments[1], message, sizeof(message));
    size_t textLength = readAll(arguments[2], text, sizeof(text) - 1);
    text[textLength] = 0;
    if (decode((const char *)text, signature, sizeof(signature)) != 64 || decode(arguments[3], key, sizeof(key)) != 32) return 1;
    return crypto_ed25519_check(signature, key, message, messageLength) == 0 ? 0 : 1;
}
