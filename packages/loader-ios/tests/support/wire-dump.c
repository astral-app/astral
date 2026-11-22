#include <stdio.h>
#include <stdlib.h>

#include "wire-parser.h"

static void print(void *context, const char *line, size_t length)
{
    (void)context;
    fwrite(line, 1, length, stdout);
    fputc('\n', stdout);
}

int main(int count, char **arguments)
{
    for (int index = 1; index < count; index++) {
        FILE *file = fopen(arguments[index], "rb");
        if (!file) continue;
        fseek(file, 0, SEEK_END);
        long length = ftell(file);
        fseek(file, 0, SEEK_SET);
        unsigned char *bytes = malloc((size_t)length);
        if (bytes && fread(bytes, 1, (size_t)length, file) == (size_t)length) astral_wire_summarise(bytes, (size_t)length, print, NULL);
        free(bytes);
        fclose(file);
    }
    return 0;
}
