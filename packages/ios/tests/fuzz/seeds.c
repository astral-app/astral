#include <stdio.h>
#include <stdlib.h>

#include "PhoenixWireInternal.h"
#include "../support/batch-builder.h"

static void save(const char *directory, const char *name, const Batch *batch)
{
    char path[1024];
    snprintf(path, sizeof path, "%s/%s", directory, name);
    FILE *file = fopen(path, "wb");
    if (!file) exit(1);
    fwrite(batch->bytes, 1, batch->length, file);
    fclose(file);
}

int main(int count, char **arguments)
{
    if (count != 2) return 2;
    static Batch batch;
    overview(&batch);
    save(arguments[1], "overview.bin", &batch);
    batch.length = 0;
    b8(&batch, 1);
    children(&batch, 5, 1, (int[]){6});
    save(arguments[1], "navigate.bin", &batch);
    batch.length = 0;
    b8(&batch, 1);
    create(&batch, 900, "text", 1);
    b16(&batch, 9);
    b8(&batch, 6);
    bstr(&batch, "message.author:1234567890");
    children(&batch, 901, 1, (int[]){900});
    save(arguments[1], "author.bin", &batch);
    chat(&batch, 8);
    save(arguments[1], "chat.bin", &batch);
    return 0;
}
