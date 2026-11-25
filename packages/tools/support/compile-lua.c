#include <stdio.h>
#include <stdlib.h>

#include "lauxlib.h"
#include "lua.h"

static int writeChunk(lua_State *state, const void *bytes, size_t size, void *file)
{
    (void)state;
    return fwrite(bytes, 1, size, (FILE *)file) == size ? 0 : 1;
}

static char *readAll(const char *path, size_t *size)
{
    FILE *file = fopen(path, "rb");
    if (!file) return NULL;
    fseek(file, 0, SEEK_END);
    long length = ftell(file);
    fseek(file, 0, SEEK_SET);
    char *bytes = length >= 0 ? malloc((size_t)length + 1) : NULL;
    if (bytes && fread(bytes, 1, (size_t)length, file) != (size_t)length) {
        free(bytes);
        bytes = NULL;
    }
    fclose(file);
    *size = bytes ? (size_t)length : 0;
    return bytes;
}

int main(int argc, char **argv)
{
    if (argc != 4) {
        fprintf(stderr, "usage: compile-lua <chunk name> <source.lua> <output.luac>\n");
        return 2;
    }
    size_t size = 0;
    char *source = readAll(argv[2], &size);
    if (!source) {
        fprintf(stderr, "compile-lua: cannot read %s\n", argv[2]);
        return 1;
    }
    lua_State *state = luaL_newstate();
    if (!state || luaL_loadbufferx(state, source, size, argv[1], "t") != LUA_OK) {
        fprintf(stderr, "compile-lua: %s\n", state ? lua_tostring(state, -1) : "out of memory");
        return 1;
    }
    FILE *output = fopen(argv[3], "wb");
    if (!output) {
        fprintf(stderr, "compile-lua: cannot write %s\n", argv[3]);
        return 1;
    }
    int failed = lua_dump(state, writeChunk, output, 0);
    failed |= fclose(output) != 0;
    lua_close(state);
    free(source);
    return failed ? 1 : 0;
}
