#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include "../../sources/PhoenixLocate.h"
#include "PhoenixProperties.h"

static void printString(const char *value)
{
    putchar('"');
    for (const char *cursor = value; *cursor; cursor++) {
        if (*cursor == '"' || *cursor == '\\') putchar('\\');
        putchar(*cursor);
    }
    putchar('"');
}

int main(int count, char **arguments)
{
    if (count != 2) {
        fprintf(stderr, "usage: compat-probe <PhoenixFatClient>\n");
        return 2;
    }
    int descriptor = open(arguments[1], O_RDONLY);
    if (descriptor < 0) return 1;
    struct stat info;
    if (fstat(descriptor, &info) != 0) return 1;
    const uint8_t *image = mmap(NULL, (size_t)info.st_size, PROT_READ, MAP_PRIVATE, descriptor, 0);
    if (image == MAP_FAILED) return 1;
    AstralImageView view = {image, (size_t)info.st_size, 0, 1};
    uint64_t opsArrived = astral_locate_function_referencing(&view, "ops batch dropped");
    uint64_t hostCall = astral_locate_host_call(&view, opsArrived);
    static const char *names[2048];
    static char storage[65536];
    size_t nameCount = astral_read_property_names(&view, names, 2048, storage, sizeof storage);
    const char *missing = NULL;
    size_t resolved = astral_wire_resolve_properties(names, nameCount, &missing);
    printf("{\"opsArrived\":%llu,\"hostCall\":%llu,\"propertyCount\":%zu,\"resolved\":%zu,\"missing\":", (unsigned long long)opsArrived,
           (unsigned long long)hostCall, nameCount, resolved);
    if (missing)
        printString(missing);
    else
        printf("null");
    printf(",\"properties\":{");
    const uint16_t *ids = (const uint16_t *)&gAstralWireProperties;
    const char *fieldNames[] = {
#define ASTRAL_WIRE_NAME(name, fallback) #name,
        ASTRAL_WIRE_PROPERTIES(ASTRAL_WIRE_NAME)
#undef ASTRAL_WIRE_NAME
    };
    size_t fields = sizeof fieldNames / sizeof fieldNames[0];
    for (size_t index = 0; index < fields; index++) {
        if (index) putchar(',');
        printString(fieldNames[index]);
        printf(":%u", missing ? 0 : ids[index]);
    }
    printf("}}\n");
    munmap((void *)image, (size_t)info.st_size);
    close(descriptor);
    return opsArrived && hostCall && !missing ? 0 : 3;
}
