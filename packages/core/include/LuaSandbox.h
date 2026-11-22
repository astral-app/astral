#pragma once

#include <cstddef>
#include <optional>
#include <string>

struct lua_State;
struct lua_Debug;

namespace loader::lua {
struct SandboxLimits {
    size_t memory = 8u << 20;
    int budgetSlices = 40;
    int sliceInstructions = 5000;
};

class Sandbox
{
  public:
    explicit Sandbox(SandboxLimits limits = {});
    ~Sandbox();
    Sandbox(const Sandbox &) = delete;
    Sandbox &operator=(const Sandbox &) = delete;

    lua_State *state() const { return luaState; }
    size_t memory() const { return used; }
    std::optional<std::string> load(const std::string &source, const std::string &chunk);
    std::optional<std::string> call(int arguments, int results);
    std::optional<std::string> run(const std::string &source, const std::string &chunk);

  private:
    static void *allocate(void *user, void *pointer, size_t old, size_t size);
    static void spendBudget(lua_State *state, lua_Debug *debug);
    std::string popError();

    SandboxLimits limits;
    lua_State *luaState = nullptr;
    size_t used = 0;
    int remaining = 0;
};
} // namespace loader::lua
