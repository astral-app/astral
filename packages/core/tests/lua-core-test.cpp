#include <cassert>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <cmath>
#include <string>
#include <limits>
#include <sys/stat.h>

#include "AstralNetworkPolicy.h"
#include "AstralPrivacy.h"
#include "AstralCutout.h"
#include "AstralPacks.h"
#include "AstralSettings.h"
#include "AstralTOML.h"
#include "AstralThemes.h"
#include "AstralSignatures.h"
#include "AstralSettingsPages.h"
#include "AstralSettingsNavigator.h"
#include "AstralLuaHost.h"
#include "../../ios/tests/support/batch-builder.h"
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
    assert(astral::isTelemetryRequest("DISCORD.com", "/API/v9/Science"));
    assert(astral::isTelemetryRequest("discord.com", "/error-reporting-proxy/ios/1/envelope/"));
    assert(astral::isTelemetryRequest("o64374.ingest.sentry.io", "/api/230973/envelope/"));
    assert(astral::isTelemetryRequest("launches.appsflyersdk.com", "/api/v6.15/iosevent"));
    assert(astral::isTelemetryRequest("img.litix.io", "/a.gif?x=1"));
    assert(astral::isTelemetryRequest("siteintercept.qualtrics.com", "/WRSiteInterceptEngine/"));
    assert(astral::isTelemetryRequest("measurement.app-measurement.com", "/a"));
    assert(astral::isTelemetryRequest("www.google-analytics.com", "/collect"));
    assert(astral::isTelemetryRequest("firebase.crashlytics.com", "/api/v1/"));
    assert(!astral::isTelemetryRequest("discord.com", "/api/v9/users/@me"));
    assert(!astral::isTelemetryRequest("discord.com", "/api/v9/channels/1/messages"));
    assert(!astral::isTelemetryRequest("example.com", "/api/v9/science"));
}

static void testOutgoingMessageRequests()
{
    using astral::outgoingMessageTarget;
    auto create = outgoingMessageTarget("discord.com", "/api/v10/channels/123/messages", "POST");
    assert(create && create->channel == "123" && create->operation == "create");
    auto edit = outgoingMessageTarget("discordapp.com", "/api/v9/channels/123/messages/456", "PATCH");
    assert(edit && edit->channel == "123" && edit->operation == "edit");
    assert(!outgoingMessageTarget("example.com", "/api/v10/channels/123/messages", "POST"));
    assert(!outgoingMessageTarget("discord.com", "/api/v10/channels/123/messages", "GET"));
    assert(!outgoingMessageTarget("discord.com", "/api/v10/channels/123/messages/456", "POST"));
    assert(!outgoingMessageTarget("discord.com", "/api/v10/channels/nope/messages", "POST"));
    assert(!outgoingMessageTarget("discord.com", "/api/v10/channels/123/messages/456/reactions", "PATCH"));
    std::string source =
        R"({"content":"before","message_reference":{"message_id":"789"},"attachments":[{"id":"0","filename":"a.png"}],"flags":4})";
    auto changed = astral::transformOutgoingMessageBody(
        "discord.com", "/api/v10/channels/123/messages/456", "PATCH", source,
        [](const astral::OutgoingMessageTarget &target, std::string_view content) -> std::optional<std::string> {
            assert(target.channel == "123" && target.operation == "edit");
            assert(content == "before");
            return "after";
        });
    assert(changed);
    auto parsed = nlohmann::json::parse(*changed);
    assert(parsed["content"] == "after");
    assert(parsed["message_reference"]["message_id"] == "789");
    assert(parsed["attachments"][0]["filename"] == "a.png");
    assert(parsed["flags"] == 4);
    assert(!astral::transformOutgoingMessageBody(
        "discord.com", "/api/v10/channels/123/messages", "POST", source,
        [](const astral::OutgoingMessageTarget &, std::string_view) { return std::optional<std::string>{}; }));
}

static void testDiagnosticPrivacy()
{
    std::string token = "abcdefghijklmnopqrstuvwxyz.abcdef.abcdefghijklmnopqrstuvwxyz";
    std::string input = "Authorization: Bearer " + token +
                        " https://example.com/route?message=private\n/Users/local/private.txt /data/user/0/com.discord/private.txt "
                        "content=private-value with spaces";
    std::string sanitized = astral::sanitizeDiagnosticText(input);
    assert(sanitized.find(token) == std::string::npos);
    assert(sanitized.find("message=private") == std::string::npos);
    assert(sanitized.find("/Users/local/private.txt") == std::string::npos);
    assert(sanitized.find("/data/user/0/com.discord/private.txt") == std::string::npos);
    assert(sanitized.find("content=private-value") == std::string::npos);
    assert(sanitized.find("with spaces") == std::string::npos);
    assert(sanitized.find("[redacted]") != std::string::npos);
    assert(sanitized.find("[private path]") != std::string::npos);
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

static void testCutoutGeometry()
{
    using astral::cutout::Frame;
    using astral::cutout::Kind;
    auto absent = astral::cutout::fromSystem(std::nullopt);
    auto invalid = astral::cutout::fromSystem(Frame{0, 0, 0, 40});
    assert(!absent && !invalid);
    auto notch = astral::cutout::fromSystem(Frame{0, 0, 200, 30});
    assert(notch && notch->kind == Kind::Notch);
    assert(std::abs(astral::cutout::cornerRadius(*notch) - 19.5) < 0.001);
    auto island = astral::cutout::fromSystem(Frame{60, 10, 180, 48});
    assert(island && island->kind == Kind::Island);
    assert(std::abs(astral::cutout::cornerRadius(*island) - 24) < 0.001);
    auto hole = astral::cutout::fromSystem(Frame{80, 20, 40, 40});
    assert(hole && hole->kind == Kind::Hole);
    auto logo = astral::cutout::logoFrame(*hole);
    assert(std::abs(logo.height - 24) < 0.001);
    assert(std::abs(logo.x - 5) < 0.01);
    assert(std::abs(logo.y - 8) < 0.001);
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

static void testSliderSettingsAndNavigation(const std::string &work)
{
    auto root = std::filesystem::path(work) / "slider-host";
    auto plugin = root / "plugins" / "sample";
    std::filesystem::create_directories(plugin);
    writeText(plugin / "manifest.toml", "id = 'astral.slider-test'\nname = 'Slider test'\nversion = '1.0'\nmain = 'main.lua'\n");
    writeText(plugin / "main.lua", "astral.settings.number('dim', 'Dim', { min = 0, max = 90, step = 5, default = 55 })");
    astral::SettingsStore settings((root / "settings.json").string());
    settings.set("astral", "developer-mode", true);
    astral::luahost::start({(root / "plugins").string()}, &settings, true, {});
    assert(astral::luahost::eval("astral.slider-test", "return astral.settings.values.dim") == "55");
    astral::SettingsPagesPlatform platform;
    platform.settings = &settings;
    astral::pages::configure(std::move(platform));
    Batch batch{};
    overview(&batch);
    AstralWireBuffer buffer{};
    astral_settings_rewrite(batch.bytes, batch.length, &buffer);
    astral_wire_buffer_free(&buffer);
    astral::navigator::configure([](std::vector<uint8_t>) {}, [](const std::function<void()> &work) { work(); });
    astral::navigator::tap("settings.row.astral.plugin-settings:astral.slider-test");
    assert(astral::navigator::open());
    astral::navigator::slidePosition("astral.slider.18.number.dim", 13);
    assert(astral::luahost::eval("astral.slider-test", "return astral.settings.values.dim") == "65");
    astral::navigator::slidePosition("astral.slider.18.number.dim", 100);
    assert(astral::luahost::eval("astral.slider-test", "return astral.settings.values.dim") == "90");
    astral::navigator::slide("astral.slider.18.number.dim", std::numeric_limits<double>::quiet_NaN());
    assert(astral::luahost::eval("astral.slider-test", "return astral.settings.values.dim") == "90");
    assert(astral::navigator::back() && !astral::navigator::open());
    astral::navigator::tap("settings.row.astral.plugin-settings:astral.slider-test");
    assert(astral_page_pop(&buffer));
    astral_wire_buffer_free(&buffer);
    assert(!astral::navigator::open() && !astral::navigator::back());
    astral::luahost::stop();
    astral::pages::configure({});
}

static void testNetworkRequests(const std::string &work)
{
    auto root = std::filesystem::path(work) / "network-host";
    auto plugin = root / "plugins" / "network";
    auto restricted = root / "plugins" / "restricted";
    std::filesystem::create_directories(plugin);
    std::filesystem::create_directories(restricted);
    writeText(
        plugin / "manifest.toml",
        "id = 'astral.network-test'\nname = 'Network test'\nversion = '1.0'\nmain = 'main.lua'\npermissions = ['network', 'storage']\n");
    writeText(plugin / "main.lua",
              "local request = astral.net.request({ url = 'https://example.com/api', method = 'patch', timeout_ms = 30000, headers = { "
              "accept = 'application/json' }, body = 'request body' }, function(response) astral.storage.set('response', response) end)\n"
              "astral.storage.set('initial', request)\n");
    writeText(restricted / "manifest.toml",
              "id = 'astral.network-restricted'\nname = 'Restricted'\nversion = '1.0'\nmain = 'main.lua'\npermissions = ['storage']\n");
    writeText(restricted / "main.lua", "return true");
    astral::SettingsStore settings((root / "settings.json").string());
    settings.set("astral", "developer-mode", true);
    uint64_t submitted = 0;
    nlohmann::json options;
    std::vector<uint64_t> cancelled;
    astral::LuaHostPlatform platform;
    platform.networkRequest = [&](uint64_t request, const nlohmann::json &requestOptions) {
        submitted = request;
        options = requestOptions;
    };
    platform.cancelNetworkRequest = [&](uint64_t request) { cancelled.push_back(request); };
    astral::luahost::start({(root / "plugins").string()}, &settings, true, std::move(platform));
    assert(submitted > 0);
    assert(options["method"] == "PATCH" && options["timeout_ms"] == 30000);
    assert(options["body"] == "request body" && options["headers"]["accept"] == "application/json");
    assert(astral::luahost::eval("astral.network-test", "return astral.storage.get('initial')") == std::to_string(submitted));
    astral::luahost::completeNetworkRequest(submitted, {{"status", 201},
                                                        {"ok", true},
                                                        {"url", "https://example.com/api"},
                                                        {"headers", {{"content-type", "application/json"}}},
                                                        {"body", "complete"}});
    assert(astral::luahost::eval("astral.network-test", "return astral.storage.get('response').body") == "complete");
    uint64_t cancelledRequest =
        std::stoull(astral::luahost::eval("astral.network-test", "return astral.net.request({ url = 'https://example.com/cancel' }, "
                                                                 "function() astral.storage.set('cancelled_callback', true) end)"));
    assert(astral::luahost::eval("astral.network-test", "return astral.net.cancel(" + std::to_string(cancelledRequest) + ")") == "true");
    assert(!cancelled.empty() && cancelled.back() == cancelledRequest);
    astral::luahost::completeNetworkRequest(cancelledRequest, {{"status", 200}, {"ok", true}, {"body", "ignored"}});
    assert(astral::luahost::eval("astral.network-test", "return astral.storage.get('cancelled_callback') == nil") == "true");
    size_t beforeInvalid = submitted;
    assert(astral::luahost::eval(
               "astral.network-test",
               "local ok = pcall(function() astral.net.request({ url = 'http://example.com' }, function() end) end); return ok") ==
           "false");
    assert(submitted == beforeInvalid);
    assert(astral::luahost::eval(
               "astral.network-restricted",
               "local ok = pcall(function() astral.net.request({ url = 'https://example.com' }, function() end) end); return ok") ==
           "false");
    assert(submitted == beforeInvalid);
    uint64_t stoppedRequest = std::stoull(astral::luahost::eval(
        "astral.network-test",
        "return astral.net.request({ url = 'https://example.com/stop' }, function() astral.storage.set('stopped_callback', true) end)"));
    astral::luahost::stop();
    assert(cancelled.back() == stoppedRequest);
    astral::luahost::completeNetworkRequest(stoppedRequest, {{"status", 200}, {"ok", true}, {"body", "ignored"}});
}

static void testOutgoingMessageTransforms(const std::string &work)
{
    auto root = std::filesystem::path(work) / "message-transform-host";
    auto first = root / "plugins" / "first";
    auto second = root / "plugins" / "second";
    auto restricted = root / "plugins" / "restricted";
    std::filesystem::create_directories(first);
    std::filesystem::create_directories(second);
    std::filesystem::create_directories(restricted);
    writeText(first / "manifest.toml", "id = 'astral.transform-first'\nname = 'First'\nversion = '1.0'\nmain = 'main.lua'\npermissions = "
                                       "['discord.messages.send', 'storage']\n");
    writeText(first / "main.lua", "astral.messages.transform(function(content, context)\n"
                                  "  astral.storage.set('context', {channel = context.channel_id, operation = context.operation})\n"
                                  "  if context.channel_id == '100' then return content:gsub('foo', 'bar') end\n"
                                  "end)\n");
    writeText(second / "manifest.toml",
              "id = 'astral.transform-second'\nname = 'Second'\nversion = '1.0'\nmain = 'main.lua'\npermissions = "
              "['discord.messages.send']\n");
    writeText(second / "main.lua", "astral.messages.transform(function(content) return content .. '!' end)\n");
    writeText(restricted / "manifest.toml",
              "id = 'astral.transform-restricted'\nname = 'Restricted'\nversion = '1.0'\nmain = 'main.lua'\npermissions = ['storage']\n");
    writeText(restricted / "main.lua", "local ok = pcall(function() astral.messages.transform(function(content) return content end) end)\n"
                                       "astral.storage.set('allowed', ok)\n");
    astral::SettingsStore settings((root / "settings.json").string());
    settings.set("astral", "developer-mode", true);
    astral::LuaHostPlatform platform;
    platform.storagePath = (root / "storage.sqlite").string();
    astral::luahost::start({(root / "plugins").string()}, &settings, true, std::move(platform));
    assert(astral::luahost::hasOutgoingMessageTransforms());
    auto create = astral::luahost::transformOutgoingMessage("100", "create", "foo message");
    assert(create && *create == "bar message!");
    auto context = astral::luahost::eval("astral.transform-first",
                                         "return astral.storage.get('context').channel .. ':' .. astral.storage.get('context').operation");
    assert(context == "100:create");
    auto edit = astral::luahost::transformOutgoingMessage("200", "edit", "foo message");
    assert(edit && *edit == "foo message!");
    assert(astral::luahost::eval("astral.transform-restricted", "return astral.storage.get('allowed')") == "false");
    astral::luahost::setEnabled("astral.transform-first", false);
    assert(astral::luahost::transformOutgoingMessage("100", "create", "foo message") == std::optional<std::string>("foo message!"));
    astral::luahost::stop();
    assert(!astral::luahost::hasOutgoingMessageTransforms());
}

static void testFilePickerAndDialogs(const std::string &work)
{
    auto root = std::filesystem::path(work) / "native-ui-host";
    auto plugin = root / "plugins" / "native-ui";
    auto restricted = root / "plugins" / "restricted";
    std::filesystem::create_directories(plugin);
    std::filesystem::create_directories(restricted);
    writeText(plugin / "manifest.toml", "id = 'astral.native-ui-test'\nname = 'Native UI test'\nversion = '1.0'\nmain = "
                                        "'main.lua'\npermissions = ['files.read', 'ui.dialogs', 'events.lifecycle', 'storage']\n");
    writeText(plugin / "main.lua",
              "astral.ui.alert('Ready', 'Choose a file')\n"
              "local request = astral.files.pick({'json', '.TOML', 'json'}, function(file, reason)\n"
              "  if file then astral.storage.set('file', {name = file.name, data = file.data})\n"
              "  else astral.storage.set('failure', reason) end\n"
              "end)\n"
              "astral.storage.set('request', request)\n"
              "local subscription = astral.events.on('app.foreground', function(event) astral.storage.set('event', event) end)\n"
              "astral.storage.set('subscription', subscription)\n");
    writeText(restricted / "manifest.toml",
              "id = 'astral.native-ui-restricted'\nname = 'Restricted'\nversion = '1.0'\nmain = 'main.lua'\npermissions = ['storage']\n");
    writeText(restricted / "main.lua", "local alertOK = pcall(function() astral.ui.alert('No', 'permission') end)\n"
                                       "local pickerOK = pcall(function() astral.files.pick({'json'}, function() end) end)\n"
                                       "local eventOK = pcall(function() astral.events.on('app.foreground', function() end) end)\n"
                                       "astral.storage.set('permissions', {alert = alertOK, picker = pickerOK, event = eventOK})\n");
    astral::SettingsStore settings((root / "settings.json").string());
    settings.set("astral", "developer-mode", true);
    std::string shownTitle;
    std::string shownMessage;
    uint64_t picked = 0;
    std::vector<std::string> pickedExtensions;
    std::vector<uint64_t> cancelled;
    astral::LuaHostPlatform platform;
    platform.storagePath = (root / "storage.sqlite").string();
    platform.showAlert = [&](const std::string &title, const std::string &message) {
        shownTitle = title;
        shownMessage = message;
    };
    platform.pickFile = [&](uint64_t request, const std::vector<std::string> &extensions) {
        picked = request;
        pickedExtensions = extensions;
        return true;
    };
    platform.cancelFilePick = [&](uint64_t request) { cancelled.push_back(request); };
    astral::luahost::start({(root / "plugins").string()}, &settings, true, std::move(platform));
    assert(shownTitle == "Ready" && shownMessage == "Choose a file");
    assert(picked > 0 && pickedExtensions == std::vector<std::string>({"json", "toml"}));
    assert(std::stoull(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('request')")) == picked);
    astral::luahost::completeFilePick(picked, "theme.json", "{\"name\":\"fixture\"}", std::nullopt);
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('file').name") == "theme.json");
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('file').data") == "{\"name\":\"fixture\"}");
    assert(astral::luahost::eval("astral.native-ui-restricted",
                                 "local p = astral.storage.get('permissions'); return tostring(p.alert) .. "
                                 "':' .. tostring(p.picker) .. ':' .. tostring(p.event)") == "false:false:false");
    uint64_t subscription = std::stoull(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('subscription')"));
    astral::luahost::emitEvent("app.foreground");
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('event')") == "app.foreground");
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.events.off(tonumber(astral.storage.get('subscription')))") ==
           "true");
    astral::luahost::eval("astral.native-ui-test", "astral.storage.remove('event')");
    astral::luahost::emitEvent("app.foreground");
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('event') == nil") == "true");
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.events.off(" + std::to_string(subscription) + ")") == "false");
    uint64_t cancelledRequest = std::stoull(astral::luahost::eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function(_, reason) astral.storage.set('cancelled_callback', reason) end); "
        "astral.storage.set('cancelled_request', request); return request"));
    assert(astral::luahost::eval("astral.native-ui-test",
                                 "return astral.files.cancel(tonumber(astral.storage.get('cancelled_request')))") == "true");
    assert(!cancelled.empty() && cancelled.back() == cancelledRequest);
    astral::luahost::completeFilePick(cancelledRequest, std::nullopt, std::nullopt, std::string("cancelled"));
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('cancelled_callback') == nil") == "true");
    uint64_t oversizedRequest = std::stoull(astral::luahost::eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function(file, reason) astral.storage.set('oversized_callback', reason) end); "
        "astral.storage.set('oversized_request', request); return request"));
    astral::luahost::completeFilePick(oversizedRequest, "large.json", std::string((1u << 20) + 1, 'x'), std::nullopt);
    assert(astral::luahost::eval("astral.native-ui-test", "return astral.storage.get('oversized_callback')") == "file exceeds 1 MiB");
    uint64_t stoppedRequest = std::stoull(astral::luahost::eval(
        "astral.native-ui-test",
        "local request = astral.files.pick({'json'}, function() end); astral.storage.set('stopped_request', request); return request"));
    astral::luahost::stop();
    assert(!cancelled.empty() && cancelled.back() == stoppedRequest);
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
    testOutgoingMessageRequests();
    testDiagnosticPrivacy();
    testStaffUser();
    testSharedPacks(argv[1]);
    testThemeParity(argv[1]);
    testSliderPayloads();
    testCutoutGeometry();
    testSingletonPageCollections();
    testSharedSignatures(argv[1], argv[2]);
    testNetworkRequests(argv[1]);
    testOutgoingMessageTransforms(argv[1]);
    testFilePickerAndDialogs(argv[1]);
    testSliderSettingsAndNavigation(argv[1]);
    puts("ok");
    return 0;
}
