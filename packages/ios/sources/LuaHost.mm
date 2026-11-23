#import "LuaHost.h"

#include "AstralSettings.h"
#include "AstralLuaHost.h"
#import "AstralState.h"
#import "CrashReports.h"
#import "DevServer.h"
#import "DiscordCache.h"
#import "FilePicker.h"
#import "FoundationJSON.h"
#import "PhoenixSettings.h"
#import "PluginSignature.h"
#import "ReactionReactors.h"
#import "NetworkRequest.h"
#import <UIKit/UIKit.h>

static NSString *Native(const std::string &value)
{
    return [NSString stringWithUTF8String:value.c_str()] ?: @"";
}

static std::string Plain(NSString *value)
{
    return value.UTF8String ?: "";
}

static std::optional<nlohmann::json> JSON(id value)
{
    if (!value) return std::nullopt;
    return AstralJSONFromObject(value);
}

static std::optional<std::string> Text(NSString *value)
{
    if (!value) return std::nullopt;
    return Plain(value);
}

static std::vector<std::string> PreferredLanguages(void)
{
    std::vector<std::string> languages;
    for (NSString *language in NSLocale.preferredLanguages) languages.push_back(Plain(language));
    return languages;
}

static astral::LuaHostPlatform Platform(void)
{
    astral::LuaHostPlatform platform;
    platform.guild = [](const std::string &guild) { return JSON(AstralDiscordGuild(Native(guild))); };
    platform.member = [](const std::string &guild, const std::string &user) {
        return JSON(AstralDiscordMember(Native(guild), Native(user)));
    };
    platform.memberNamed = [](const std::string &guild, const std::string &name) {
        return Text(AstralDiscordMemberNamed(Native(guild), Native(name)));
    };
    platform.channelGuild = [](const std::string &channel) { return Text(AstralDiscordChannelGuild(Native(channel))); };
    platform.user = [](const std::string &user) { return JSON(AstralDiscordUser(Native(user))); };
    platform.friendNickname = [](const std::string &user) { return Text(AstralDiscordFriendNickname(Native(user))); };
    platform.messageChannel = [](const std::string &message) { return Text(AstralDiscordMessageChannel(Native(message))); };
    platform.message = [](const std::string &channel, const std::string &message) {
        return JSON(AstralDiscordMessage(Native(channel), Native(message)));
    };
    platform.permissions = [](const std::string &guild, const std::string &user,
                              const std::optional<std::string> &channel) -> std::optional<uint64_t> {
        NSNumber *permissions = AstralDiscordPermissions(Native(guild), Native(user), channel ? Native(*channel) : nil);
        if (!permissions) return std::nullopt;
        return permissions.unsignedLongLongValue;
    };
    platform.reactors = [](const std::string &channel, const std::string &message, const std::string &emoji, bool burst, uint64_t count,
                           bool &fresh) {
        BOOL current = NO;
        NSArray *users = AstralReactors(Native(channel), Native(message), Native(emoji), burst, (NSUInteger)count, &current);
        fresh = current;
        return JSON(users);
    };
    platform.networkRequest = [](uint64_t request, const nlohmann::json &options) { astral::startNetworkRequest(request, options); };
    platform.cancelNetworkRequest = [](uint64_t request) { astral::cancelNetworkRequest(request); };
    platform.showAlert = [](const std::string &title, const std::string &message) { AstralShowAlert(Native(title), Native(message)); };
    platform.writeClipboard = [](const std::string &value) {
        dispatch_async(dispatch_get_main_queue(), ^{
            UIPasteboard.generalPasteboard.string = Native(value);
        });
        return true;
    };
    platform.pickFile = [](uint64_t request, const std::vector<std::string> &extensions) {
        NSMutableArray<NSString *> *types = [NSMutableArray arrayWithCapacity:extensions.size()];
        for (const auto &extension : extensions) [types addObject:Native(extension)];
        return AstralPickPluginFile(request, types, ^(uint64_t completedRequest, NSData *data, NSString *name, NSString *error) {
            std::optional<std::string> fileName;
            std::optional<std::string> contents;
            std::optional<std::string> failure;
            if (name) fileName = Plain(name);
            if (data) contents = std::string(static_cast<const char *>(data.bytes), data.length);
            if (error) failure = Plain(error);
            astral::luahost::completeFilePick(completedRequest, fileName, contents, failure);
        });
    };
    platform.cancelFilePick = [](uint64_t request) { AstralCancelPluginFile(request); };
    platform.verifySignature = [](const std::string &directory, const std::string &main) {
        return (bool)AstralVerifyPluginDirectory(Native(directory), Native(main));
    };
    platform.recordFailure = [](const std::string &identifier, const std::string &name, const std::string &error) {
        AstralRecordPluginFailure(Native(identifier), Native(name), Native(error));
    };
    platform.developerLog = [](const std::string &line) { AstralDevAppendLog(Native(line)); };
    platform.refreshDecorations = [] { AstralPhoenixRefreshDecorations(); };
    platform.preferredLanguages = PreferredLanguages();
    platform.storagePath = Plain([AstralStateDirectory() stringByAppendingPathComponent:@"plugin-storage.sqlite"]);
    platform.bundledDirectory = Plain(AstralResourceDirectory());
    return platform;
}

void AstralLuaHostStart(NSArray<NSString *> *directories, astral::SettingsStore *store, bool startEnabled)
{
    std::vector<std::string> roots;
    for (NSString *directory in directories) roots.push_back(Plain(directory));
    astral::luahost::start(roots, store, startEnabled, Platform());
}

void AstralLuaHostEmitEvent(NSString *event)
{
    astral::luahost::emitEvent(Plain(event));
}

NSArray<NSDictionary *> *AstralLuaPlugins(void)
{
    return AstralObjectFromJSON(astral::luahost::plugins()) ?: @[];
}

void AstralLuaRemoveStorage(NSString *identifier)
{
    astral::luahost::removeStorage(Plain(identifier));
}

void AstralLuaSetEnabled(NSString *pluginID, BOOL enabled)
{
    astral::luahost::setEnabled(Plain(pluginID), enabled);
}

void AstralLuaSetSwitch(NSString *pluginID, NSString *key, BOOL on)
{
    astral::luahost::setSwitch(Plain(pluginID), Plain(key), on);
}

void AstralLuaSetChoice(NSString *pluginID, NSString *key, NSString *value)
{
    astral::luahost::setChoice(Plain(pluginID), Plain(key), Plain(value));
}

void AstralLuaSetNumber(NSString *pluginID, NSString *key, double value)
{
    astral::luahost::setNumber(Plain(pluginID), Plain(key), value);
}

void AstralLuaRescan(void)
{
    astral::luahost::rescan();
}

void AstralLuaRemove(NSString *pluginID)
{
    astral::luahost::remove(Plain(pluginID));
}

void AstralLuaReload(NSString *pluginID)
{
    astral::luahost::reload(Plain(pluginID));
}

void AstralLuaResetSettings(NSString *pluginID)
{
    astral::luahost::resetSettings(Plain(pluginID));
}

NSString *AstralLuaEval(NSString *pluginID, NSString *code)
{
    return Native(astral::luahost::eval(Plain(pluginID), Plain(code)));
}
