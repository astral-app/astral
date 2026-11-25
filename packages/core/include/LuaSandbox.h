#pragma once

#include <cstddef>
#include <optional>
#include <string>

#include "AstralRust.h"

struct lua_State;

extern "C" {
struct AstralLuaSandbox;
AstralLuaSandbox *astral_lua_sandbox_new(size_t memory, int budgetSlices, int sliceInstructions);
void astral_lua_sandbox_free(AstralLuaSandbox *sandbox);
lua_State *astral_lua_sandbox_state(const AstralLuaSandbox *sandbox);
size_t astral_lua_sandbox_memory(const AstralLuaSandbox *sandbox);
AstralRustString astral_lua_sandbox_load(AstralLuaSandbox *sandbox, const char *source, size_t length, const char *chunk,
                                         size_t chunkLength, int bytecode);
AstralRustString astral_lua_sandbox_call(AstralLuaSandbox *sandbox, int arguments, int results);
}

namespace astral::lua {
struct SandboxLimits {
    size_t memory = 8u << 20;
    int budgetSlices = 40;
    int sliceInstructions = 5000;
};

class Sandbox
{
  public:
    explicit Sandbox(SandboxLimits limits = {})
        : handle(astral_lua_sandbox_new(limits.memory, limits.budgetSlices, limits.sliceInstructions))
    {
    }
    ~Sandbox() { astral_lua_sandbox_free(handle); }
    Sandbox(const Sandbox &) = delete;
    Sandbox &operator=(const Sandbox &) = delete;

    lua_State *state() const { return astral_lua_sandbox_state(handle); }
    size_t memory() const { return astral_lua_sandbox_memory(handle); }

    std::optional<std::string> load(const std::string &source, const std::string &chunk)
    {
        return rust::takeIfPresent(astral_lua_sandbox_load(handle, source.data(), source.size(), chunk.data(), chunk.size(), 0));
    }

    std::optional<std::string> loadBytecode(const std::string &bytecode, const std::string &chunk)
    {
        return rust::takeIfPresent(astral_lua_sandbox_load(handle, bytecode.data(), bytecode.size(), chunk.data(), chunk.size(), 1));
    }

    std::optional<std::string> call(int arguments, int results)
    {
        return rust::takeIfPresent(astral_lua_sandbox_call(handle, arguments, results));
    }

    std::optional<std::string> run(const std::string &source, const std::string &chunk)
    {
        if (auto error = load(source, chunk)) return error;
        return call(0, 0);
    }

  private:
    AstralLuaSandbox *handle;
};
} // namespace astral::lua
