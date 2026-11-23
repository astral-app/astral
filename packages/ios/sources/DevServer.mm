#import "DevServer.h"

#include "AstralDevServer.h"
#include "AstralSettings.h"
#import "AstralState.h"
#import "Backup.h"
#import "EmojiComposer.h"
#import "EmojiPacks.h"
#import "FoundationJSON.h"
#import "IconPacks.h"
#import "NetworkFilter.h"
#import "SoftRestart.h"
#import "Themes.h"

using astral::dev::Request;
using astral::dev::Response;

static void (^gChanged)(void);

static NSString *Native(const std::string &value)
{
    return [NSString stringWithUTF8String:value.c_str()] ?: @"";
}

static std::string Plain(NSString *value)
{
    return value.UTF8String ?: "";
}

static std::optional<std::string> StringField(const Request &request, const char *name)
{
    if (!request.json.is_object() || !request.json.contains(name) || !request.json[name].is_string()) return std::nullopt;
    return request.json[name].get<std::string>();
}

static void OnMain(void (^work)(void))
{
    if (NSThread.isMainThread)
        work();
    else
        dispatch_sync(dispatch_get_main_queue(), work);
}

static void Changed(void)
{
    if (gChanged) gChanged();
}

static Response Backup(void)
{
    __block NSString *text = nil;
    OnMain(^{
        text = AstralBackupTOML();
    });
    return {200, {{"backup", Plain(text ?: @"")}}};
}

static Response Restore(const std::string &backup)
{
    __block NSString *failure = nil;
    NSData *data = [Native(backup) dataUsingEncoding:NSUTF8StringEncoding];
    OnMain(^{
        failure = AstralApplyBackup(data, gChanged
                                              ?: ^{
                                                 });
    });
    if (failure) return {400, {{"error", Plain(failure)}}};
    return {200, {{"imported", true}}};
}

static Response Themes(void)
{
    nlohmann::json themes = nlohmann::json::array();
    for (NSDictionary *theme in AstralThemes()) themes.push_back(Plain(theme[@"id"] ?: @""));
    return {200, {{"active", Plain(AstralActiveThemeID())}, {"themes", themes}}};
}

static Response ApplyTheme(const std::string &identifier)
{
    OnMain(^{
        AstralApplyTheme(Native(identifier));
        Changed();
    });
    return {200, {{"active", Plain(AstralActiveThemeID())}}};
}

static Response Gateway(const std::string &message)
{
    BOOL queued = AstralInjectGatewayMessage(Native(message));
    return {queued ? 200 : 400, {{"queued", (bool)queued}, {"needs", "Staff DevTools on, for an uncompressed gateway"}}};
}

static Response EmojiPack(const std::string &identifier)
{
    OnMain(^{
        AstralSelectEmojiPack(Native(identifier));
        AstralRepaintEmojiComposers();
    });
    return {200, {{"active", Plain(AstralActiveEmojiPackID())}, {"animated", (bool)AstralEmojiAnimated()}}};
}

static Response SoftRestart(void (^before)(void), const char *extraKey, NSString * (^extraValue)(void))
{
    __block NSDictionary *result = nil;
    OnMain(^{
        if (before) before();
        result = AstralSoftRestart();
    });
    nlohmann::json reply = AstralJSONFromObject(result);
    if (!reply.is_object()) reply = nlohmann::json::object();
    reply[extraKey] = Plain(extraValue());
    return {200, reply};
}

static Response UnboundTheme(const std::string &body)
{
    __block NSString *failure = nil;
    NSData *data = [NSData dataWithBytes:body.data() length:body.size()];
    OnMain(^{
        failure = AstralImportUnboundTheme(data, @"Imported");
        Changed();
    });
    if (failure) return {400, {{"error", Plain(failure)}}};
    return {200, {{"imported", true}}};
}

static std::optional<Response> Route(const Request &request)
{
    const std::string &method = request.method, &route = request.route;
    if (method == "GET" && route == "/backup") return Backup();
    if (method == "POST" && route == "/backup") {
        if (auto backup = StringField(request, "backup")) return Restore(*backup);
    }
    if (method == "GET" && route == "/themes") return Themes();
    if (method == "POST" && route == "/theme") {
        if (auto identifier = StringField(request, "id")) return ApplyTheme(*identifier);
    }
    if (method == "POST" && route == "/gateway" && !request.body.empty()) return Gateway(request.body);
    if (method == "POST" && route == "/emoji-pack") {
        if (auto identifier = StringField(request, "id")) return EmojiPack(*identifier);
    }
    if (method == "POST" && route == "/icon-pack") {
        if (auto identifier = StringField(request, "id")) {
            NSString *selected = Native(*identifier);
            return SoftRestart(
                ^{
                    AstralSelectIconPack(selected);
                },
                "active",
                ^{
                    return AstralActiveIconPackID();
                });
        }
    }
    if (method == "POST" && route == "/reload") {
        return SoftRestart(
            ^{
                Changed();
            },
            "theme",
            ^{
                return AstralActiveThemeID();
            });
    }
    if (method == "POST" && route == "/unbound-theme" && !request.body.empty()) return UnboundTheme(request.body);
    return std::nullopt;
}

void AstralDevAppendLog(NSString *line)
{
    astral::dev::appendLog(Plain(line));
}

NSString *AstralDevPairingCode(void)
{
    return Native(astral::dev::pairingCode());
}

void AstralStartDevServer(BOOL (^enabled)(void), void (^changed)(void))
{
    gChanged = [changed copy];
    BOOL (^isEnabled)(void) = [enabled copy];
    astral::dev::Hooks hooks;
    hooks.enabled = [isEnabled] { return (bool)isEnabled(); };
    hooks.onMain = [](const std::function<void()> &work) {
        std::function<void()> copy = work;
        OnMain(^{
            copy();
        });
    };
    hooks.changed = [] { Changed(); };
    hooks.status = [] { return nlohmann::json{{"network", AstralJSONFromObject(AstralNetworkFilterStats())}}; };
    hooks.routes = Route;
    hooks.version = Plain(AstralVersion());
    hooks.stateDirectory = Plain(AstralStateDirectory());
    hooks.settings = AstralSettings();
    astral::dev::start(std::move(hooks));
}
