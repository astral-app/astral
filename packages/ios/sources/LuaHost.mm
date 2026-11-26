#import "LuaHost.h"

#import <AVFoundation/AVFoundation.h>
#import <UserNotifications/UserNotifications.h>
#import "LuaComposer.h"

#include "AstralSettings.h"
#include "AstralLuaHost.h"
#import "AstralState.h"
#import "BrowserAuth.h"
#import "ConfirmSheet.h"
#import "NotificationTaps.h"
#import "PluginSheet.h"
#import "CrashReports.h"
#import "DevServer.h"
#import "DiscordCache.h"
#import "FilePicker.h"
#import "FoundationJSON.h"
#import "PhoenixSettings.h"
#import "PickedMedia.h"
#import "PickOffer.h"
#include "PhoenixWire.h"
#import "PluginSignature.h"
#import "ReactionReactors.h"
#import "NetworkRequest.h"
#import "NetworkFilter.h"
#import "ImageRenderer.h"
#import <UIKit/UIKit.h>
#import <objc/message.h>
#import <objc/runtime.h>

static UIView *IdentifiedView(UIView *view, NSString *identifier, BOOL prefix)
{
    if (view.hidden || view.alpha < 0.01) return nil;
    NSString *own = view.accessibilityIdentifier;
    if (own && (prefix ? [own hasPrefix:identifier] : [own isEqualToString:identifier])) return view;
    for (UIView *child in view.subviews.reverseObjectEnumerator) {
        UIView *found = IdentifiedView(child, identifier, prefix);
        if (found) return found;
    }
    return nil;
}

static BOOL FireTapRecognizer(UIView *view, BOOL ancestors)
{
    for (UIView *current = view; current; current = ancestors ? current.superview : nil) {
        for (UIGestureRecognizer *recognizer in current.gestureRecognizers) {
            if (![recognizer isKindOfClass:UITapGestureRecognizer.class] || !recognizer.enabled) continue;
            Ivar targetsIvar = class_getInstanceVariable(UIGestureRecognizer.class, "_targets");
            NSArray *targets = targetsIvar ? object_getIvar(recognizer, targetsIvar) : nil;
            BOOL fired = NO;
            for (id entry in targets) {
                Ivar targetIvar = class_getInstanceVariable([entry class], "_target");
                Ivar actionIvar = class_getInstanceVariable([entry class], "_action");
                if (!targetIvar || !actionIvar) continue;
                id target = object_getIvar(entry, targetIvar);
                SEL action = ((SEL (*)(id, Ivar))object_getIvar)(entry, actionIvar);
                if (!target || !action || ![target respondsToSelector:action]) continue;
                SEL setState = NSSelectorFromString(@"setState:");
                BOOL settable = [recognizer respondsToSelector:setState];
                @try {
                    if (settable) ((void (*)(id, SEL, NSInteger))objc_msgSend)(recognizer, setState, UIGestureRecognizerStateEnded);
                    ((void (*)(id, SEL, id))objc_msgSend)(target, action, recognizer);
                    fired = YES;
                } @catch (NSException *exception) {
                    AstralDevAppendLog(
                        [NSString stringWithFormat:@"press: %@ refused the tap: %@", NSStringFromClass([target class]), exception.reason]);
                }
                if (settable) ((void (*)(id, SEL, NSInteger))objc_msgSend)(recognizer, setState, UIGestureRecognizerStatePossible);
            }
            if (fired) return YES;
        }
    }
    return NO;
}

static BOOL PressIdentifiedView(NSString *identifier, BOOL prefix)
{
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows.reverseObjectEnumerator) {
            UIView *view = IdentifiedView(window, identifier, prefix);
            if (!view) continue;
            if (FireTapRecognizer(view, NO) || [view accessibilityActivate] || FireTapRecognizer(view, YES)) return YES;
            if ([view isKindOfClass:UIControl.class]) {
                [(UIControl *)view sendActionsForControlEvents:UIControlEventTouchUpInside];
                return YES;
            }
            return NO;
        }
    }
    return NO;
}

static NSData *SilentAudio(void)
{
    const uint32_t sampleRate = 8000, samples = sampleRate;
    NSMutableData *wave = [NSMutableData data];
    auto append32 = ^(uint32_t value) {
        [wave appendBytes:&value length:4];
    };
    auto append16 = ^(uint16_t value) {
        [wave appendBytes:&value length:2];
    };
    [wave appendBytes:"RIFF" length:4];
    append32(36 + samples * 2);
    [wave appendBytes:"WAVEfmt " length:8];
    append32(16);
    append16(1);
    append16(1);
    append32(sampleRate);
    append32(sampleRate * 2);
    append16(2);
    append16(16);
    [wave appendBytes:"data" length:4];
    append32(samples * 2);
    [wave increaseLengthBy:samples * 2];
    return wave;
}

static AVAudioPlayer *gKeepAlivePlayer;
static bool gKeepAliveWanted;

static bool PlaySilence(void)
{
    AVAudioSession *session = AVAudioSession.sharedInstance;
    if (session.category != AVAudioSessionCategoryPlayback || !(session.categoryOptions & AVAudioSessionCategoryOptionMixWithOthers))
        [session setCategory:AVAudioSessionCategoryPlayback withOptions:AVAudioSessionCategoryOptionMixWithOthers error:nil];
    [session setActive:YES error:nil];
    if (!gKeepAlivePlayer) {
        gKeepAlivePlayer = [[AVAudioPlayer alloc] initWithData:SilentAudio() error:nil];
        gKeepAlivePlayer.numberOfLoops = -1;
        gKeepAlivePlayer.volume = 0;
    }
    return gKeepAlivePlayer.playing || [gKeepAlivePlayer play];
}

static void ReassertSilenceLater(NSNotification *note)
{
    if ([note.name isEqualToString:AVAudioSessionMediaServicesWereResetNotification]) gKeepAlivePlayer = nil;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        if (gKeepAliveWanted) PlaySilence();
    });
}

static void WatchAudioSession(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
        for (NSNotificationName name in @[
                 UIApplicationDidEnterBackgroundNotification, AVAudioSessionInterruptionNotification,
                 AVAudioSessionMediaServicesWereResetNotification
             ])
            [center addObserverForName:name
                                object:nil
                                 queue:NSOperationQueue.mainQueue
                            usingBlock:^(NSNotification *note) {
                                ReassertSilenceLater(note);
                            }];
    });
}

static bool SetKeepAlive(bool on)
{
    gKeepAliveWanted = on;
    if (!on) {
        [gKeepAlivePlayer stop];
        gKeepAlivePlayer = nil;
        return true;
    }
    WatchAudioSession();
    return PlaySilence();
}

static void PostNotification(UNNotificationRequest *request)
{
    [UNUserNotificationCenter.currentNotificationCenter
        addNotificationRequest:request
         withCompletionHandler:^(NSError *error) {
             if (error)
                 AstralDevAppendLog(
                     [NSString stringWithFormat:@"Failed to post notification %@: %@", request.identifier, error.localizedDescription]);
         }];
}

static void PostNotificationWhenAuthorized(UNNotificationRequest *request)
{
    UNUserNotificationCenter *center = UNUserNotificationCenter.currentNotificationCenter;
    [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
        if (settings.authorizationStatus != UNAuthorizationStatusNotDetermined) {
            PostNotification(request);
            return;
        }
        UNAuthorizationOptions options = UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
        [center requestAuthorizationWithOptions:options
                              completionHandler:^(BOOL granted, NSError *error) {
                                  if (granted)
                                      PostNotification(request);
                                  else
                                      AstralDevAppendLog([NSString stringWithFormat:@"Failed to get notification permission for %@: %@",
                                                                                    request.identifier,
                                                                                    error.localizedDescription ?: @"denied"]);
                              }];
    }];
}

static std::string AppLink(const std::string &url)
{
    static const std::string kChannels = "https://discord.com/channels/";
    if (url.rfind(kChannels, 0) != 0) return url;
    return "discord://-/channels/" + url.substr(kChannels.size());
}

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

static AVSpeechSynthesizer *AstralSpeech(void)
{
    static AVSpeechSynthesizer *synthesizer;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        synthesizer = [AVSpeechSynthesizer new];
    });
    return synthesizer;
}

static void AstralKeepPlayer(AVAudioPlayer *player)
{
    static AVAudioPlayer *current;
    [current stop];
    current = player;
}

static astral::LuaHostPlatform Platform(void)
{
    astral::LuaHostPlatform platform;
    platform.discordDirectory =
        [NSFileManager.defaultManager URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject.path.UTF8String
            ?: "";
    platform.reactors = [](const std::string &channel, const std::string &message, const std::string &emoji, bool burst, uint64_t count,
                           bool &fresh) {
        BOOL current = NO;
        NSArray *users = AstralReactors(Native(channel), Native(message), Native(emoji), burst, (NSUInteger)count, &current);
        fresh = current;
        return JSON(users);
    };
    platform.networkRequest = [](uint64_t request, const nlohmann::json &options) { astral::startNetworkRequest(request, options); };
    platform.cancelNetworkRequest = [](uint64_t request) { astral::cancelNetworkRequest(request); };
    platform.webAuth = [](uint64_t request, const std::string &url, const std::string &callbackURL) {
        return AstralOpenPluginWebAuth(request, Native(url), Native(callbackURL),
                                       ^(uint64_t completedRequest, NSString *resultURL, NSString *cookies, NSString *error) {
                                           astral::luahost::completeWebAuth(completedRequest, Text(resultURL), Text(cookies), Text(error));
                                       });
    };
    platform.cancelWebAuth = [](uint64_t request) { AstralCancelPluginWebAuth(request); };
    platform.showAlert = [](const std::string &title, const std::string &message) { AstralShowAlert(Native(title), Native(message)); };
    platform.openSheet = [](uint64_t sheet, const nlohmann::json &spec) {
        NSDictionary *options = AstralObjectFromJSON(spec);
        if (![options isKindOfClass:NSDictionary.class]) return false;
        return static_cast<bool>(AstralOpenSheet(sheet, options, ^(NSString *action, NSString *value, BOOL closed) {
            std::string chosen = Plain(action), text = Plain(value);
            dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                astral::luahost::sheetEvent(sheet, chosen, text, closed);
            });
        }));
    };
    platform.updateSheet = [](uint64_t sheet, const nlohmann::json &spec) {
        NSDictionary *options = AstralObjectFromJSON(spec);
        if ([options isKindOfClass:NSDictionary.class]) AstralUpdateSheet(sheet, options);
    };
    platform.closeSheet = [](uint64_t sheet) { AstralCloseSheet(sheet); };
    platform.confirm = [](uint64_t request, const nlohmann::json &spec) {
        NSDictionary *options = @{
            @"title" : Native(spec.value("title", std::string())),
            @"message" : Native(spec.value("message", std::string())),
            @"image" : Native(spec.value("image", std::string())),
            @"confirm" : Native(spec.value("confirm", std::string("OK"))),
            @"cancel" : Native(spec.value("cancel", std::string("Cancel"))),
            @"destructive" : @(spec.value("destructive", false)),
        };
        return static_cast<bool>(AstralShowConfirm(options, ^(BOOL accepted) {
            dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                astral::luahost::completeConfirm(request, accepted);
            });
        }));
    };
    platform.showToast = [](const std::string &title, const std::string &message) { AstralShowToast(Native(title), Native(message)); };
    platform.triggerHaptic = [](const std::string &strength) {
        UIImpactFeedbackStyle style;
        if (strength == "light")
            style = UIImpactFeedbackStyleLight;
        else if (strength == "medium")
            style = UIImpactFeedbackStyleMedium;
        else if (strength == "heavy")
            style = UIImpactFeedbackStyleHeavy;
        else
            return false;
        dispatch_async(dispatch_get_main_queue(), ^{
            UIImpactFeedbackGenerator *generator = [[UIImpactFeedbackGenerator alloc] initWithStyle:style];
            [generator prepare];
            [generator impactOccurred];
        });
        return true;
    };
    platform.writeClipboard = [](const std::string &value) {
        NSString *text = Native(value);
        dispatch_async(dispatch_get_main_queue(), ^{
            UIPasteboard.generalPasteboard.string = text;
        });
        return true;
    };
    platform.readClipboard = []() -> std::optional<std::string> {
        __block NSString *value = nil;
        if (NSThread.isMainThread) {
            value = UIPasteboard.generalPasteboard.string;
        } else {
            dispatch_semaphore_t read = dispatch_semaphore_create(0);
            dispatch_async(dispatch_get_main_queue(), ^{
                value = UIPasteboard.generalPasteboard.string;
                dispatch_semaphore_signal(read);
            });
            if (dispatch_semaphore_wait(read, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC))) return std::nullopt;
        }
        if (!value) return std::nullopt;
        return std::string(value.UTF8String ?: "");
    };
    platform.notify = [](const std::string &identifier, const std::string &title, const std::string &body, double delay,
                         const std::string &link) {
        UNMutableNotificationContent *content = [UNMutableNotificationContent new];
        content.title = Native(title);
        content.body = Native(body);
        content.sound = UNNotificationSound.defaultSound;
        if (!link.empty()) {
            content.userInfo = @{AstralNotificationLinkKey : Native(AppLink(link))};
            AstralRouteNotificationTaps();
        }
        UNTimeIntervalNotificationTrigger *trigger =
            delay >= 1 ? [UNTimeIntervalNotificationTrigger triggerWithTimeInterval:delay repeats:NO] : nil;
        UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:Native(identifier) content:content trigger:trigger];
        PostNotificationWhenAuthorized(request);
        return true;
    };
    platform.pickedMedia = [](const std::string &slot) -> std::optional<astral::PickedMedia> {
        unsigned long long index = strtoull(slot.c_str() + 1, nullptr, 10);
        if (index == 0) return std::nullopt;
        int64_t size = -1;
        double duration = 0;
        NSString *type = nil;
        if (!AstralPickedMediaInfo((NSUInteger)(index - 1), &size, &duration, &type, ^{
                AstralPhoenixRefreshDecorations();
            }))
            return std::nullopt;
        return astral::PickedMedia{size, duration, type.UTF8String ?: "unknown"};
    };
    platform.pickedVideoURL = [](const std::string &slot) -> std::optional<std::string> {
        unsigned long long index = strtoull(slot.c_str() + 1, nullptr, 10);
        if (index == 0) return std::nullopt;
        NSString *url = AstralPickedVideoURL((NSUInteger)(index - 1));
        if (!url) return std::nullopt;
        return std::string(url.UTF8String);
    };
    platform.offerPick = [](const std::string &data, const std::string &name) {
        return (bool)AstralOfferPick([NSData dataWithBytes:data.data() length:data.size()], Native(name));
    };
    platform.speak = [](const std::string &text, const std::string &language, double rate) {
        AVSpeechUtterance *utterance = [AVSpeechUtterance speechUtteranceWithString:Native(text)];
        utterance.rate =
            AVSpeechUtteranceMinimumSpeechRate + (AVSpeechUtteranceMaximumSpeechRate - AVSpeechUtteranceMinimumSpeechRate) * rate;
        if (!language.empty()) utterance.voice = [AVSpeechSynthesisVoice voiceWithLanguage:Native(language)];
        dispatch_async(dispatch_get_main_queue(), ^{
            [AstralSpeech() speakUtterance:utterance];
        });
        return true;
    };
    platform.playSound = [](const std::string &path, double volume) {
        NSURL *file = [NSURL fileURLWithPath:Native(path)];
        dispatch_async(dispatch_get_main_queue(), ^{
            NSError *error = nil;
            AVAudioPlayer *player = [[AVAudioPlayer alloc] initWithContentsOfURL:file error:&error];
            if (!player) {
                AstralDevAppendLog(
                    [NSString stringWithFormat:@"Failed to play %@: %@", file.lastPathComponent, error.localizedDescription]);
                return;
            }
            player.volume = volume;
            AstralKeepPlayer(player);
            [player play];
        });
        return true;
    };
    platform.stopAudio = [] {
        dispatch_async(dispatch_get_main_queue(), ^{
            [AstralSpeech() stopSpeakingAtBoundary:AVSpeechBoundaryImmediate];
            AstralKeepPlayer(nil);
        });
    };
    platform.openURL = [](const std::string &url) {
        NSURL *link = [NSURL URLWithString:Native(AppLink(url))];
        if (!link) return false;
        dispatch_async(dispatch_get_main_queue(), ^{
            [UIApplication.sharedApplication openURL:link options:@{} completionHandler:nil];
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
    platform.keepAlive = [](bool on) {
        __block bool result = false;
        if (NSThread.isMainThread)
            result = SetKeepAlive(on);
        else
            dispatch_sync(dispatch_get_main_queue(), ^{
                result = SetKeepAlive(on);
            });
        return result;
    };
    platform.insertComposerText = [](const std::string &text, bool replace) {
        NSString *value = Native(text);
        dispatch_async(dispatch_get_main_queue(), ^{
            AstralInsertComposerText(value, replace);
        });
        return true;
    };
    platform.renderImage = [](const nlohmann::json &spec) -> std::optional<std::string> {
        NSData *json = [NSData dataWithBytes:spec.dump().data() length:spec.dump().size()];
        NSDictionary *parsed = [NSJSONSerialization JSONObjectWithData:json options:0 error:nil];
        if (![parsed isKindOfClass:NSDictionary.class]) return std::nullopt;
        __block NSData *png = nil;
        if (NSThread.isMainThread)
            png = AstralRenderImage(parsed);
        else
            dispatch_sync(dispatch_get_main_queue(), ^{
                png = AstralRenderImage(parsed);
            });
        if (!png) return std::nullopt;
        return std::string((const char *)png.bytes, png.length);
    };
    platform.environment = [] {
        NSData *data = [NSJSONSerialization dataWithJSONObject:AstralDiagnosticEnvironment() options:0 error:nil];
        nlohmann::json environment =
            data ? nlohmann::json::parse(std::string((const char *)data.bytes, data.length), nullptr, false) : nlohmann::json::object();
        return environment.is_object() ? environment : nlohmann::json::object();
    };
    platform.pressNode = [](const std::string &identifier, bool prefix) {
        NSString *wanted = Native(identifier);
        if ([NSThread isMainThread]) return (bool)PressIdentifiedView(wanted, prefix);
        __block BOOL pressed = NO;
        dispatch_sync(dispatch_get_main_queue(), ^{
            pressed = PressIdentifiedView(wanted, prefix);
        });
        return (bool)pressed;
    };
    platform.dispatchGateway = [](const std::string &message) { return (bool)AstralDispatchLocalGatewayEvent(Native(message)); };
    platform.scrollToMessage = [](uint64_t message, bool animated, const std::string &align) {
        if (!astral_wire_scroll_to_message(message, animated, align.c_str())) return false;
        AstralPhoenixRefreshDecorations();
        return true;
    };
    platform.focusComposer = [](bool focus) {
        if (!astral_wire_focus_composer(focus)) return false;
        AstralPhoenixRefreshDecorations();
        return true;
    };
    platform.dismissSheets = [] {
        dispatch_async(dispatch_get_main_queue(), ^{
            for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
                if (![scene isKindOfClass:UIWindowScene.class]) continue;
                for (UIWindow *window in ((UIWindowScene *)scene).windows) {
                    if (window.isKeyWindow && window.rootViewController.presentedViewController)
                        [window.rootViewController dismissViewControllerAnimated:YES completion:nil];
                }
            }
        });
        return true;
    };
    platform.refreshDecorations = [] {
        AstralPhoenixRefreshDecorations();
        AstralRepaintLuaComposers();
    };
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

NSArray<NSDictionary *> *AstralLuaComposerStyles(NSString *text)
{
    return AstralObjectFromJSON(astral::luahost::composerStyles(Plain(text))) ?: @[];
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
