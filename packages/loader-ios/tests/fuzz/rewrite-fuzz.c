#include <stdint.h>
#include <stdlib.h>

#include "../../sources/PhoenixWire.c"
#include "../support/rewriter-setup.h"

int LLVMFuzzerInitialize(int *count, char ***arguments)
{
    (void)count;
    (void)arguments;
    setupRewriter();
    return 0;
}

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
    AstralWireBuffer out;
    if (astral_settings_rewrite(data, size, &out)) astral_wire_buffer_free(&out);
    return 0;
}
