#include <cassert>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>
#include <limits>
#include <sys/stat.h>

#include "AstralNetworkPolicy.h"
#include "AstralPacks.h"
#include "AstralSettings.h"
#include "AstralTOML.h"
#include "AstralThemes.h"
#include "AstralSignatures.h"
#include "AstralSettingsPages.h"
#include "AstralFiles.h"
#include "PhoenixProperties.h"
#include "PhoenixWire.h"

#ifdef __APPLE__
#include <CommonCrypto/CommonDigest.h>
#else
#include <openssl/sha.h>
#endif
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

static void testTelemetryRequests()
{
    assert(astral::isTelemetryRequest("discord.com", "/api/v9/science"));
    assert(astral::isTelemetryRequest("discord.com", "/api/v10/metrics/v2?x=1"));
    assert(astral::isTelemetryRequest("o64374.ingest.sentry.io", "/api/230973/envelope/"));
    assert(!astral::isTelemetryRequest("discord.com", "/api/v9/users/@me"));
    assert(!astral::isTelemetryRequest("discord.com", "/api/v9/channels/1/messages"));
    assert(!astral::isTelemetryRequest("example.com", "/api/v9/science"));
}

static void testStaffUser()
{
    assert(astral::isCurrentUserRequest("discord.com", "/api/v9/users/@me"));
    assert(!astral::isCurrentUserRequest("discord.com", "/api/v9/users/@me/settings"));
    auto user = nlohmann::json::parse(*astral::withStaffUser(R"({"id":"1","flags":64})"));
    assert(user["flags"] == 65 && user["public_flags"] == 1);
    assert(!astral::withStaffUser("[1]"));
    auto ready = nlohmann::json::parse(*astral::withStaffReady(R"({"op":0,"t":"READY","d":{"user":{"id":"1","flags":0}}})"));
    assert(ready["d"]["user"]["flags"] == 1);
    assert(!astral::withStaffReady(R"({"op":0,"t":"MESSAGE_CREATE","d":{"content":"READY"}})"));
}

static void testSharedPacks(const std::string &work)
{
    auto root = std::filesystem::path(work) / "packs";
    auto bundled = root / "bundled";
    auto local = root / "local";
    std::filesystem::create_directories(local);
    std::filesystem::create_directories(bundled / "emoji-packs" / "sample");
    std::filesystem::create_directories(local / "emoji-packs" / "sample");
    std::filesystem::create_directories(bundled / "icon-packs" / "sample" / "icons");
    writeText(bundled / "emoji-packs" / "sample" / "manifest.toml",
              "id = 'sample'\nname = 'Bundled'\nstatic = 'https://example.com/bundled/{codepoints}.png'\n");
    writeText(local / "emoji-packs" / "sample" / "manifest.toml",
              "id = 'sample'\nname = 'Local'\nstatic = 'https://example.com/static/{codepoints}.png'\nanimated = "
              "'https://example.com/animated/{codepoints}.gif'\n");
    writeText(local / "emoji-packs" / "sample" / "coverage.txt", "1f600\r\n1f469+1f4bb\n2764!\n");
    writeText(local / "emoji-packs" / "sample" / "animated.txt", "1f600\n");
    writeText(bundled / "icon-packs" / "sample" / "manifest.toml", "id = 'sample'\nname = 'Icons'\n");
    writeText(bundled / "icon-packs" / "sample" / "icons" / "ic_test@2x.png", "two");
    astral::SettingsStore settings((local / "settings.json").string());
    astral::Packs packs(settings, local.string(), bundled.string());
    assert(packs.emoji().size() == 2 && packs.emoji()[1]["name"] == "Local");
    assert(packs.emojiURL("1f600").empty());
    assert(!packs.selectEmoji("missing"));
    assert(packs.selectEmoji("sample"));
    assert(packs.selectEmoji("sample"));
    assert(packs.emojiURL("1F600") == "https://example.com/animated/1f600.gif");
    packs.setAnimated(false);
    assert(packs.emojiURL("1f600") == "https://example.com/static/1f600.png");
    assert(packs.emojiURL("1f469-200d-1f4bb") == "https://example.com/static/1f469-200d-1f4bb.png");
    assert(packs.emojiURL("2764-fe0f") == "https://example.com/static/2764-fe0f.png");
    assert(packs.emojiURL("1f601").empty());
    assert(packs.emojiURL("../../secret").empty());
    assert(packs.selectIcons("sample"));
    assert(packs.iconPath("ic_test") == (bundled / "icon-packs" / "sample" / "icons" / "ic_test@2x.png").string());
    assert(packs.iconPath("ic_../../secret").empty());
    assert(packs.selectIcons("discord") && packs.iconPath("ic_test").empty());
    assert(packs.selectEmoji("discord") && packs.emojiURL("1f600").empty());
    assert(astral::parseTOML("invalid = [").is_null());
}

static void testThemeParity(const std::string &work)
{
    auto root = std::filesystem::path(work) / "themes";
    auto bundled = root / "bundled";
    auto local = root / "local";
    std::filesystem::create_directories(local);
    std::filesystem::create_directories(bundled / "themes");
    std::filesystem::create_directories(bundled / "palettes");
    writeText(bundled / "themes" / "sample.toml",
              "id = 'sample'\nname = 'Sample'\n[colors]\nFF202020 = 'FF302040'\n[tint]\ncolor = '#8B5CF6'\n[tint.text]\ncolor = "
              "'#EEEEFF'\nsaturation = 0.2\n[background]\ngradient = ['#112233', '#445566']\n");
    writeText(bundled / "palettes" / "discord-raw.toml", "[colors]\nDARK = '#202020'\nLIGHT = '#EEEEEE'\n");
    writeText(bundled / "palettes" / "discord-semantic.toml", "[colors.background]\ndark = ['DARK', 1]\nlight = ['LIGHT', 1]\n");
    astral::SettingsStore settings((local / "settings.json").string());
    astral::Themes themes(settings, local.string(), bundled.string());
    auto style = themes.select("sample");
    assert(style.from.size() == 2 && style.to.size() == 2);
    assert(style.from[0] == 0xFF202020 && style.from[1] == 0xFFEEEEEE);
    assert(style.to[0] == 0xFF302040 && style.to[1] == 0xFF302040);
    assert(style.tint == 0xFF8B5CF6 && style.surfaces[kAstralSurfaceText] == 0xFFEEEEFF);
    assert(style.gradient == std::vector<uint32_t>({0xFF112233, 0xFF445566}));
    assert(themes.active() == "sample");
    assert(themes.select("missing").from.empty() && themes.active() == "discord");
    assert(!themes.importUnbound(R"({"name":"My theme","semanticColors":{"background":["#ABCDEF"]}})", "fallback"));
    assert(themes.active() == "unbound-my-theme");
    assert(!themes.importUnbound(R"({"name":"My theme","semanticColors":{"background":["#AABBCC"]}})", "fallback"));
    auto imported = themes.select(themes.active());
    assert(imported.to[0] == 0xFFAABBCC);
    assert(themes.importUnbound("[]", "invalid"));
    astral::Packs packs(settings, local.string(), bundled.string());
    astral::SettingsPagesPlatform platform;
    platform.settings = &settings;
    platform.themes = [&] { return themes.list(); };
    platform.activeTheme = [&] { return themes.active(); };
    platform.iconPacks = [&] { return packs.icons(); };
    platform.activeIconPack = [&] { return packs.activeIcons(); };
    platform.emojiPacks = [&] { return packs.emoji(); };
    platform.activeEmojiPack = [&] { return packs.activeEmoji(); };
    astral::pages::configure(std::move(platform));
    auto design = astral::pages::model("design");
    for (const auto &section : design["sections"]) {
        assert(section["rows"].is_array());
        for (const auto &entry : section["rows"]) assert(entry.is_object());
    }
    bool converted = false;
    astral::pages::withSpec(design, [&](const AstralPageSpec *spec) {
        assert(std::string(spec->title) == "Design" && spec->count == 5);
        converted = true;
    });
    assert(converted);
    astral::pages::configure({});
}

static void testSliderPayloads()
{
    auto valueKey = P(value);
    std::vector<uint8_t> payload{8, 1, 0, 0, 0, static_cast<uint8_t>(valueKey), static_cast<uint8_t>(valueKey >> 8), 3, 7, 0, 0, 0};
    double value = 0;
    assert(astral_wire_event_value(payload.data(), payload.size(), &value) && value == 7);
    for (size_t size = 0; size < payload.size(); size++) assert(!astral_wire_event_value(payload.data(), size, &value));
    payload.push_back(0);
    assert(!astral_wire_event_value(payload.data(), payload.size(), &value));
    payload.pop_back();
    payload[7] = 5;
    payload.resize(16);
    double invalid = std::numeric_limits<double>::quiet_NaN();
    memcpy(payload.data() + 8, &invalid, sizeof(invalid));
    assert(!astral_wire_event_value(payload.data(), payload.size(), &value));
}

static void testSingletonPageCollections()
{
    nlohmann::json row = {{"key", "theme-note"}, {"title", "Theme note"}, {"kind", kAstralRowInfo}};
    nlohmann::json model = {{"title", "Design"}, {"sections", {{"title", "How themes apply"}, {"rows", row}}}};
    bool converted = false;
    astral::pages::withSpec(model, [&](const AstralPageSpec *spec) {
        assert(spec->count == 1 && spec->sections[0].count == 1);
        assert(std::string(spec->sections[0].rows[0].title) == "Theme note");
        converted = true;
    });
    assert(converted);
    model["sections"] = nlohmann::json::array({"invalid", {{"rows", nlohmann::json::array({row, "invalid"})}}});
    astral::pages::withSpec(model, [](const AstralPageSpec *spec) { assert(spec->count == 1 && spec->sections[0].count == 1); });
}

static void testSharedSignatures(const std::string &work, const std::string &repository)
{
    auto key = astral::readTextFile(repository + "/keys/plugin-signing.pub");
    assert(key);
    astral::Signatures verifier(*key, [](const std::string &data) {
        uint8_t bytes[32];
#ifdef __APPLE__
        CC_SHA256(data.data(), static_cast<CC_LONG>(data.size()), bytes);
#else
        SHA256(reinterpret_cast<const uint8_t *>(data.data()), data.size(), bytes);
#endif
        std::string digest;
        const char *digits = "0123456789abcdef";
        for (auto byte : bytes) {
            digest += digits[byte >> 4];
            digest += digits[byte & 15];
        }
        return digest;
    });
    auto catalog = astral::readTextFile(repository + "/packages/catalog/index.toml");
    auto signature = astral::readTextFile(repository + "/packages/catalog/index.toml.sig");
    assert(catalog && signature && verifier.verify(*catalog, *signature));
    assert(!verifier.verify(*catalog + "tampered", *signature));
    assert(!verifier.verify(*catalog, "invalid"));
    auto plugin = repository + "/packages/addons/plugins/show-me-your-name";
    auto directory = std::filesystem::path(work) / "signed-plugin";
    std::filesystem::copy(plugin, directory, std::filesystem::copy_options::recursive);
    assert(verifier.directory(directory.string(), "main.lua"));
    writeText(directory / "unsigned.lua", "return true");
    assert(!verifier.directory(directory.string(), "main.lua"));
    std::filesystem::remove(directory / "unsigned.lua");
    writeText(directory / "main.lua", "return true");
    assert(!verifier.directory(directory.string(), "main.lua"));
    auto forged = astral::readTextFile(plugin + "/signature");
    assert(forged && !verifier.files(*forged + "\n[files.bad]\nvalue = 'x'\n"));
    writeText(directory / "signature", std::string((128u << 10) + 1, 'x'));
    assert(!verifier.directory(directory.string(), "main.lua"));
    std::filesystem::remove(directory / "signature");
    assert(mkfifo((directory / "signature").c_str(), 0600) == 0);
    assert(!verifier.directory(directory.string(), "main.lua"));
    std::filesystem::remove(directory / "signature");
}

int main(int argc, char **argv)
{
    assert(argc == 3);
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
    testTelemetryRequests();
    testStaffUser();
    testSharedPacks(argv[1]);
    testThemeParity(argv[1]);
    testSliderPayloads();
    testSingletonPageCollections();
    testSharedSignatures(argv[1], argv[2]);
    puts("ok");
    return 0;
}
