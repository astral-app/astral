#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <string.h>

#include "../sources/PhoenixLocate.h"
#include "PhoenixProperties.h"

int main(int count, char **arguments)
{
    if (count != 2) return 2;
    int descriptor = open(arguments[1], O_RDONLY);
    assert(descriptor >= 0);
    struct stat info;
    fstat(descriptor, &info);
    const uint8_t *image = mmap(NULL, (size_t)info.st_size, PROT_READ, MAP_PRIVATE, descriptor, 0);
    assert(image != MAP_FAILED);
    AstralImageView view = {image, (size_t)info.st_size, 0, 1};
    uint64_t offset = astral_locate_function_referencing(&view, "ops batch dropped");
    printf("opsArrived at 0x%llx\n", (unsigned long long)offset);
    assert(offset == 0x333ce8);
    assert(astral_locate_function_referencing(&view, "no such diagnostic anywhere") == 0);
    uint64_t hostCall = astral_locate_host_call(&view, offset);
    printf("host call at 0x%llx\n", (unsigned long long)hostCall);
    assert(hostCall == 0x7cf00);
    assert(astral_locate_unique_caller(&view, 0) == 0);
    static const char *names[2048];
    static char storage[65536];
    size_t nameCount = astral_read_property_names(&view, names, 2048, storage, sizeof storage);
    printf("%zu property names\n", nameCount);
    assert(nameCount >= 800);
    assert(strcmp(names[0], "absolute") == 0 && strcmp(names[9], "accessibilityId") == 0 && strcmp(names[499], "text") == 0);
    assert(strcmp(names[577], "colors") == 0 && strcmp(names[613], "positions") == 0);
    AstralWireProperties fallback = gAstralWireProperties;
    const char *missing = NULL;
    size_t found = astral_wire_resolve_properties(names, nameCount, &missing);
    if (missing) printf("missing %s\n", missing);
    assert(!missing && found == sizeof(AstralWireProperties) / sizeof(uint16_t));
    assert(memcmp(&fallback, &gAstralWireProperties, sizeof fallback) == 0);
    return 0;
}
