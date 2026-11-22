#include <cassert>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>

#include "LuaJSON.h"
#include "LuaLocale.h"
#include "LuaSandbox.h"
#include "PluginPackage.h"
#include "PluginStorage.h"

extern "C" {
#include "lua.h"
}

using astral::lua::Sandbox;

static void testSandboxRunsCode()
{
    Sandbox sandbox;
    assert(!sandbox.run("value = 6 * 7", "test"));
    lua_getglobal(sandbox.state(), "value");
    assert(lua_tointeger(sandbox.state(), -1) == 42);
}

static void testSandboxHidesUnsafeGlobals()
{
    Sandbox sandbox;
    assert(!sandbox.run("assert(io == nil and os == nil and load == nil and require == nil and dofile == nil)", "test"));
}

static void testSandboxStopsRunawayLoops()
{
    Sandbox sandbox;
    auto error = sandbox.run("while true do end", "test");
    assert(error && error->find("instruction budget") != std::string::npos);
    assert(!sandbox.run("value = 1", "test"));
}

static void testSandboxCapsMemory()
{
    Sandbox sandbox({.memory = 512u << 10});
    auto error = sandbox.run("local parts = {} for index = 1, 1e6 do parts[index] = ('x'):rep(64) .. index end", "test");
    assert(error);
    assert(sandbox.memory() <= 512u << 10);
}

static void testSandboxReportsSyntaxErrors()
{
    Sandbox sandbox;
    auto error = sandbox.load("local =", "broken");
    assert(error && error->find("broken") != std::string::npos);
}

static void testJSONRoundTrip()
{
    Sandbox sandbox;
    nlohmann::json value = {{"name", "astral"}, {"count", 3}, {"ratio", 0.5}, {"tags", {"a", "b"}}, {"on", true}};
    astral::lua::pushJSON(sandbox.state(), value);
    nlohmann::json back;
    assert(astral::lua::toJSON(sandbox.state(), -1, back));
    assert(back == value);
}

static void testJSONRejectsFunctionsAndDeepTables()
{
    Sandbox sandbox;
    assert(!sandbox.run("callable = { run = function() end } deep = {{{{{{{{{{}}}}}}}}}}", "test"));
    nlohmann::json out;
    lua_getglobal(sandbox.state(), "callable");
    assert(!astral::lua::toJSON(sandbox.state(), -1, out));
    lua_getglobal(sandbox.state(), "deep");
    assert(!astral::lua::toJSON(sandbox.state(), -1, out));
}

static void testBestLocale()
{
    using astral::lua::bestLocale;
    assert(bestLocale({"en-US", "de"}, {"de-AT"}) == "de");
    assert(bestLocale({"en-GB", "fr"}, {"ja"}) == "en-GB");
    assert(bestLocale({"pt-BR"}, {"pt-BR"}) == "pt-BR");
    assert(!bestLocale({"fr"}, {"ja"}));
}

static void testFillPlaceholders()
{
    assert(astral::lua::fillPlaceholders("{count} of {count} by {name}", {{"count", "2"}, {"name", "{count}"}}) == "2 of 2 by {count}");
}

static void testPluginIdentifiers()
{
    using astral::lua::validPluginIdentifier;
    assert(validPluginIdentifier("dev.astral.pronouns"));
    assert(!validPluginIdentifier("ab"));
    assert(!validPluginIdentifier("dev..astral"));
    assert(!validPluginIdentifier("Dev.Astral"));
}

static void testPluginStorage(const std::string &directory)
{
    std::string path = directory + "/storage.sqlite";
    std::filesystem::remove(path);
    astral::PluginStorage storage(path, 64);
    assert(storage.available());
    assert(storage.set("one", "count", 3) == astral::StorageResult::Saved);
    assert(storage.set("one", "name", "astral") == astral::StorageResult::Saved);
    assert(storage.get("one", "count") == nlohmann::json(3));
    assert(!storage.get("two", "count"));
    assert((storage.keys("one") == std::vector<std::string>{"count", "name"}));
    assert(storage.set("one", "big", std::string(80, 'x')) == astral::StorageResult::OverLimit);
    assert(storage.set("one", "count", nullptr) == astral::StorageResult::Removed);
    assert(!storage.get("one", "count"));
    assert(storage.removeAll("one"));
    assert(storage.keys("one").empty());
}

static void writeText(const std::filesystem::path &path, const std::string &text)
{
    std::filesystem::create_directories(path.parent_path());
    std::ofstream(path) << text;
}

static void testPluginPackage(const std::string &directory)
{
    std::filesystem::path root = std::filesystem::path(directory) / "plugin";
    std::filesystem::remove_all(root);
    writeText(root / "manifest.toml", "id = \"astral.sample\"\nname = \"Sample\"\nversion = \"1.0\"\nauthors = [\"adrian\", { name = "
                                      "\"rae\", id = \"42\" }]\npermissions = [\"storage\"]\n");
    writeText(root / "main.lua", "return 1");
    writeText(root / "lib" / "util.lua", "return {}");
    writeText(root / "tests" / "sample.lua", "print(1)");
    writeText(root / "i18n" / "en-US.toml", "HELLO = \"Hello {name}\"\nONLY = \"English only\"\n");
    writeText(root / "i18n" / "de.toml", "HELLO = \"Hallo {name}\"\n");
    auto package = astral::loadPluginPackage(root.string(), {"de-AT"});
    assert(package);
    assert(package->identifier == "astral.sample" && package->version == "1.0" && package->main == "main.lua");
    assert(package->authors.size() == 2 && package->authors[1].second == "42");
    assert(package->permissions.count("storage"));
    assert(package->modules.size() == 1 && package->modules.count("lib.util"));
    assert(package->locale == "de");
    assert(package->translate("HELLO", {{"name", "astral"}}) == "Hallo astral");
    assert(package->translate("ONLY", {}) == "English only");
    assert(package->translate("MISSING", {}) == "MISSING");
    writeText(root / "manifest.toml", "id = \"Bad Id\"\nname = \"Sample\"\n");
    assert(!astral::loadPluginPackage(root.string(), {}));
}

static void testPluginHealth()
{
    astral::PluginHealth health;
    assert(!health.recordError("start", "boom", 2));
    assert(health.recordError("start", "again", 2) && health.failed && health.lastError == "start: again");
    assert(!health.markFailed());
    health.reset();
    assert(!health.failed && health.errors == 0 && health.lastError.empty());
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    testSandboxRunsCode();
    testSandboxHidesUnsafeGlobals();
    testSandboxStopsRunawayLoops();
    testSandboxCapsMemory();
    testSandboxReportsSyntaxErrors();
    testJSONRoundTrip();
    testJSONRejectsFunctionsAndDeepTables();
    testBestLocale();
    testFillPlaceholders();
    testPluginIdentifiers();
    testPluginStorage(argv[1]);
    testPluginPackage(argv[1]);
    testPluginHealth();
    puts("ok");
    return 0;
}
