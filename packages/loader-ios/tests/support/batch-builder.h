#pragma once
#include <stddef.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    unsigned char bytes[65536];
    size_t length;
} Batch;

static inline void b8(Batch *b, unsigned v)
{
    b->bytes[b->length++] = (unsigned char)v;
}
static inline void b16(Batch *b, unsigned v)
{
    b8(b, v & 255);
    b8(b, v >> 8);
}
static inline void b32(Batch *b, int v)
{
    memcpy(b->bytes + b->length, &v, 4);
    b->length += 4;
}
static inline void bstr(Batch *b, const char *s)
{
    b32(b, (int)strlen(s));
    memcpy(b->bytes + b->length, s, strlen(s));
    b->length += strlen(s);
}

static inline void create(Batch *b, int id, const char *type, int props)
{
    b8(b, 1);
    b32(b, id);
    b16(b, (unsigned)strlen(type));
    memcpy(b->bytes + b->length, type, strlen(type));
    b->length += strlen(type);
    b16(b, (unsigned)props);
}

static inline void children(Batch *b, int parent, int count, const int *ids)
{
    b8(b, 3);
    b32(b, parent);
    b32(b, count);
    for (int i = 0; i < count; i++) b32(b, ids[i]);
}

static inline void skeleton(Batch *b)
{
    create(b, 5, "navigator", 0);
    create(b, 50, "navigator", 0);
    create(b, 51, "view", 0);
    create(b, 6, "view", 0);
    const int plain[] = {100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 112, 113, 115, 116};
    for (size_t i = 0; i < sizeof(plain) / sizeof(*plain); i++) create(b, plain[i], "view", 0);
    create(b, 111, "view", 2);
    b16(b, 9);
    b8(b, 6);
    bstr(b, "Back");
    b16(b, 740);
    b8(b, 12);
    b32(b, 0);
    b16(b, 0);
    create(b, 114, "image", 1);
    b16(b, 542);
    b8(b, 6);
    bstr(b, "res://ic_mana_x_small");
    create(b, 117, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Settings");
    create(b, 30, "view", 0);
    create(b, 31, "view", 0);
}

static inline void links(Batch *b)
{
    children(b, 30, 1, (int[]){31});
    children(b, 100, 1, (int[]){101});
    children(b, 101, 1, (int[]){102});
    children(b, 102, 1, (int[]){103});
    children(b, 103, 4, (int[]){104, 105, 106, 107});
    children(b, 106, 1, (int[]){10});
    children(b, 107, 2, (int[]){108, 115});
    children(b, 108, 1, (int[]){109});
    children(b, 109, 1, (int[]){110});
    children(b, 110, 1, (int[]){111});
    children(b, 111, 1, (int[]){112});
    children(b, 112, 1, (int[]){113});
    children(b, 113, 1, (int[]){114});
    children(b, 115, 1, (int[]){116});
    children(b, 116, 1, (int[]){117});
    children(b, 5, 2, (int[]){6, 100});
    children(b, 50, 1, (int[]){51});
    b8(b, 2);
    b32(b, 5);
    b16(b, 1);
    b16(b, 252);
    b8(b, 3);
    b32(b, 1);
}

static inline void overview(Batch *b)
{
    b->length = 0;
    b8(b, 1);
    skeleton(b);
    create(b, 10, "view", 0);
    create(b, 11, "input", 0);
    create(b, 20, "view", 1);
    b16(b, 147);
    b8(b, 6);
    bstr(b, "column");
    create(b, 21, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Account Settings");
    create(b, 22, "view", 0);
    create(b, 23, "view", 4);
    b16(b, 9);
    b8(b, 6);
    bstr(b, "settings.row.account");
    b16(b, 11);
    b8(b, 6);
    bstr(b, "Account");
    b16(b, 320);
    b8(b, 7);
    b32(b, 1);
    b8(b, 6);
    bstr(b, "press");
    b16(b, 733);
    b8(b, 11);
    b32(b, 2);
    b32(b, 7);
    b32(b, 9);
    create(b, 24, "view", 0);
    create(b, 25, "image", 1);
    b16(b, 542);
    b8(b, 6);
    bstr(b, "res://ic_mana_user_circle");
    create(b, 26, "text", 1);
    b16(b, 499);
    b8(b, 6);
    bstr(b, "Account");
    create(b, 27, "view", 0);
    create(b, 28, "image", 0);
    children(b, 27, 1, (int[]){28});
    children(b, 24, 3, (int[]){25, 26, 27});
    children(b, 23, 1, (int[]){24});
    children(b, 22, 2, (int[]){23, 30});
    children(b, 20, 2, (int[]){21, 22});
    children(b, 10, 2, (int[]){11, 20});
    links(b);
}

static inline void chat(Batch *b, int rows)
{
    b->length = 0;
    b8(b, 1);
    char identifier[64];
    for (int row = 0; row < rows; row++) {
        int base = 2000 + row * 4;
        create(b, base, "view", 1);
        b16(b, 9);
        b8(b, 6);
        bstr(b, "message-row");
        create(b, base + 1, "text", 3);
        snprintf(identifier, sizeof identifier, "message.author:%d", 100000 + row);
        b16(b, 9);
        b8(b, 6);
        bstr(b, identifier);
        b16(b, 499);
        b8(b, 6);
        bstr(b, "someone");
        b16(b, 129);
        b8(b, 4);
        b32(b, (int)0xFFFFFFFF);
        b32(b, 0);
        create(b, base + 2, "image", 1);
        snprintf(identifier, sizeof identifier, "https://cdn.discordapp.com/avatars/%d/a.png", 500000 + row);
        b16(b, 542);
        b8(b, 6);
        bstr(b, identifier);
        create(b, base + 3, "view", 0);
        children(b, base, 3, (int[]){base + 1, base + 2, base + 3});
    }
}
