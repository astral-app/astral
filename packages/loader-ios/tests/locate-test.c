#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include "../sources/PhoenixLocate.h"

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
    return 0;
}
