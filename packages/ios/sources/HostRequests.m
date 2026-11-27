#import "HostRequests.h"

#import "AstralRequestKinds.h"

#import <AVFoundation/AVFoundation.h>
#import <UIKit/UIKit.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/message.h>
#import <objc/runtime.h>

#import "AstralCore.h"
#import "SyncFolder.h"
#import "BrowserAuth.h"
#import "ConfirmSheet.h"
#import "EmojiComposer.h"
#import "EnvironmentReport.h"
#import "FilePicker.h"
#import "LuaComposer.h"
#import "NetworkFilter.h"
#import "NotificationTaps.h"
#import "PhoenixImage.h"
#import "PhoenixSettings.h"
#import "PickOffer.h"
#import "PickedMedia.h"
#import "PluginSheet.h"

static const double kPermille = 1000;
static const uint32_t kSilenceSampleRate = 8000;

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
    const uint32_t samples = kSilenceSampleRate;
    NSMutableData *wave = [NSMutableData data];
    void (^append32)(uint32_t) = ^(uint32_t value) {
        [wave appendBytes:&value length:sizeof value];
    };
    void (^append16)(uint16_t) = ^(uint16_t value) {
        [wave appendBytes:&value length:sizeof value];
    };
    [wave appendBytes:"RIFF" length:4];
    append32(36 + samples * 2);
    [wave appendBytes:"WAVEfmt " length:8];
    append32(16);
    append16(1);
    append16(1);
    append32(kSilenceSampleRate);
    append32(kSilenceSampleRate * 2);
    append16(2);
    append16(16);
    [wave appendBytes:"data" length:4];
    append32(samples * 2);
    [wave increaseLengthBy:samples * 2];
    return wave;
}

static AVAudioPlayer *gKeepAlivePlayer;
static BOOL gKeepAliveWanted;

static BOOL PlaySilence(void)
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

static void WatchAudioSession(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        for (NSNotificationName name in @[
                 UIApplicationDidEnterBackgroundNotification, AVAudioSessionInterruptionNotification,
                 AVAudioSessionMediaServicesWereResetNotification
             ])
            [NSNotificationCenter.defaultCenter
                addObserverForName:name
                            object:nil
                             queue:NSOperationQueue.mainQueue
                        usingBlock:^(NSNotification *note) {
                            if ([note.name isEqualToString:AVAudioSessionMediaServicesWereResetNotification]) gKeepAlivePlayer = nil;
                            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), dispatch_get_main_queue(), ^{
                                if (gKeepAliveWanted) PlaySilence();
                            });
                        }];
    });
}

static BOOL SetKeepAlive(BOOL on)
{
    gKeepAliveWanted = on;
    if (!on) {
        [gKeepAlivePlayer stop];
        gKeepAlivePlayer = nil;
        return YES;
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

static NSString *AppLink(NSString *url)
{
    static NSString *const kChannels = @"https://discord.com/channels/";
    if (![url hasPrefix:kChannels]) return url;
    return [@"discord://-/channels/" stringByAppendingString:[url substringFromIndex:kChannels.length]];
}

static AVSpeechSynthesizer *Speech(void)
{
    static AVSpeechSynthesizer *synthesizer;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        synthesizer = [AVSpeechSynthesizer new];
    });
    return synthesizer;
}

static void KeepPlayer(AVAudioPlayer *player)
{
    static AVAudioPlayer *current;
    [current stop];
    current = player;
}

static id OnMain(id (^work)(void))
{
    if (NSThread.isMainThread) return work();
    __block id result = nil;
    dispatch_sync(dispatch_get_main_queue(), ^{
        result = work();
    });
    return result;
}

static id JSONOf(AstralText text)
{
    NSData *data = AstralDataOfText(text);
    return data.length ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
}

static NSDictionary *DictionaryOf(AstralText text)
{
    id value = JSONOf(text);
    return [value isKindOfClass:NSDictionary.class] ? value : nil;
}

static AstralReply Reply(AstralReplyKind kind, BOOL flag, NSData *data)
{
    AstralReply reply = {kind, (bool)flag, NULL, 0};
    if (!data) return reply;
    reply.bytes = (uint8_t *)malloc(data.length ? data.length : 1);
    if (!reply.bytes) return reply;
    memcpy(reply.bytes, data.bytes, data.length);
    reply.length = data.length;
    return reply;
}

static AstralReply Done(void)
{
    return Reply(AstralReplyKind_Done, NO, nil);
}

static AstralReply Flag(BOOL flag)
{
    return Reply(AstralReplyKind_Flag, flag, nil);
}

static AstralReply Text(NSString *text)
{
    return Reply(AstralReplyKind_Text, NO, [text dataUsingEncoding:NSUTF8StringEncoding]);
}

static AstralReply Unsupported(void)
{
    return Reply(AstralReplyKind_Unsupported, NO, nil);
}

static AstralReply Haptic(NSString *strength)
{
    NSDictionary<NSString *, NSNumber *> *styles = @{
        @"light" : @(UIImpactFeedbackStyleLight),
        @"medium" : @(UIImpactFeedbackStyleMedium),
        @"heavy" : @(UIImpactFeedbackStyleHeavy),
    };
    NSNumber *style = styles[strength];
    if (!style) return Flag(NO);
    dispatch_async(dispatch_get_main_queue(), ^{
        UIImpactFeedbackGenerator *generator = [[UIImpactFeedbackGenerator alloc] initWithStyle:(UIImpactFeedbackStyle)style.integerValue];
        [generator prepare];
        [generator impactOccurred];
    });
    return Flag(YES);
}

static AstralReply ReadClipboard(void)
{
    __block NSString *value = nil;
    if (NSThread.isMainThread) {
        value = UIPasteboard.generalPasteboard.string;
    } else {
        dispatch_semaphore_t read = dispatch_semaphore_create(0);
        dispatch_async(dispatch_get_main_queue(), ^{
            value = UIPasteboard.generalPasteboard.string;
            dispatch_semaphore_signal(read);
        });
        if (dispatch_semaphore_wait(read, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC))) return Text(nil);
    }
    return Text(value);
}

static AstralReply OpenLink(NSString *url)
{
    NSURL *link = [NSURL URLWithString:AppLink(url)];
    if (!link) return Flag(NO);
    dispatch_async(dispatch_get_main_queue(), ^{
        [UIApplication.sharedApplication openURL:link options:@{} completionHandler:nil];
    });
    return Flag(YES);
}

static AstralReply Notify(const AstralRequest *request)
{
    NSDictionary *payload = DictionaryOf(request->second) ?: @{};
    UNMutableNotificationContent *content = [UNMutableNotificationContent new];
    content.title = AstralStringOfText(request->first);
    content.body = [payload[@"body"] isKindOfClass:NSString.class] ? payload[@"body"] : @"";
    content.sound = UNNotificationSound.defaultSound;
    NSString *link = [payload[@"link"] isKindOfClass:NSString.class] ? payload[@"link"] : @"";
    if (link.length) {
        content.userInfo = @{AstralNotificationLinkKey : AppLink(link)};
        AstralRouteNotificationTaps();
    }
    double delay = request->number / kPermille;
    UNTimeIntervalNotificationTrigger *trigger =
        delay >= 1 ? [UNTimeIntervalNotificationTrigger triggerWithTimeInterval:delay repeats:NO] : nil;
    PostNotificationWhenAuthorized([UNNotificationRequest requestWithIdentifier:NSUUID.UUID.UUIDString content:content trigger:trigger]);
    return Flag(YES);
}

static AstralReply Speak(const AstralRequest *request)
{
    AVSpeechUtterance *utterance = [AVSpeechUtterance speechUtteranceWithString:AstralStringOfText(request->first)];
    double rate = request->number / kPermille;
    utterance.rate = AVSpeechUtteranceMinimumSpeechRate + (AVSpeechUtteranceMaximumSpeechRate - AVSpeechUtteranceMinimumSpeechRate) * rate;
    NSString *language = AstralStringOfText(request->second);
    if (language.length) utterance.voice = [AVSpeechSynthesisVoice voiceWithLanguage:language];
    dispatch_async(dispatch_get_main_queue(), ^{
        [Speech() speakUtterance:utterance];
    });
    return Flag(YES);
}

static AstralReply PlaySound(const AstralRequest *request)
{
    NSURL *file = [NSURL fileURLWithPath:AstralStringOfText(request->first)];
    float volume = request->number / kPermille;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSError *error = nil;
        AVAudioPlayer *player = [[AVAudioPlayer alloc] initWithContentsOfURL:file error:&error];
        if (!player) {
            AstralDevAppendLog([NSString stringWithFormat:@"Failed to play %@: %@", file.lastPathComponent, error.localizedDescription]);
            return;
        }
        player.volume = volume;
        KeepPlayer(player);
        [player play];
    });
    return Flag(YES);
}

static AstralReply StopAudio(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        [Speech() stopSpeakingAtBoundary:AVSpeechBoundaryImmediate];
        KeepPlayer(nil);
    });
    return Done();
}

static AstralReply WebAuth(const AstralRequest *request)
{
    BOOL opened =
        AstralOpenPluginWebAuth(request->identifier, AstralStringOfText(request->first), AstralStringOfText(request->second),
                                ^(uint64_t completed, NSString *url, NSString *cookies, NSString *error) {
                                    astral_ios_complete_web_auth(completed, AstralTextOf(url), AstralTextOf(cookies), AstralTextOf(error));
                                });
    return Flag(opened);
}

static NSString *TextField(NSDictionary *spec, NSString *key, NSString *fallback)
{
    id value = spec[key];
    return [value isKindOfClass:NSString.class] ? value : fallback;
}

static AstralReply Confirm(const AstralRequest *request)
{
    NSDictionary *spec = DictionaryOf(request->first) ?: @{};
    NSDictionary *options = @{
        @"title" : TextField(spec, @"title", @""),
        @"message" : TextField(spec, @"message", @""),
        @"image" : TextField(spec, @"image", @""),
        @"confirm" : TextField(spec, @"confirm", @"OK"),
        @"cancel" : TextField(spec, @"cancel", @"Cancel"),
        @"destructive" : @([spec[@"destructive"] boolValue]),
    };
    uint64_t identifier = request->identifier;
    return Flag(AstralShowConfirm(options, ^(BOOL accepted) {
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            astral_ios_complete_confirm(identifier, accepted);
        });
    }));
}

static AstralReply OpenSheet(const AstralRequest *request)
{
    NSDictionary *spec = DictionaryOf(request->first);
    if (!spec) return Flag(NO);
    uint64_t sheet = request->identifier;
    return Flag(AstralOpenSheet(sheet, spec, ^(NSString *action, NSString *value, BOOL closed) {
        astral_ios_sheet_event(sheet, AstralTextOf(action), AstralTextOf(value), closed);
    }));
}

static AstralReply UpdateSheet(const AstralRequest *request)
{
    NSDictionary *spec = DictionaryOf(request->first);
    if (spec) AstralUpdateSheet(request->identifier, spec);
    return Done();
}

static AstralReply PickFile(const AstralRequest *request)
{
    NSMutableArray<NSString *> *types = [NSMutableArray array];
    for (NSString *extension in [AstralStringOfText(request->first) componentsSeparatedByString:@"\n"]) {
        if (extension.length) [types addObject:extension];
    }
    return Flag(AstralPickPluginFile(request->identifier, types, ^(uint64_t completed, NSData *data, NSString *name, NSString *error) {
        astral_ios_complete_file_pick(completed, AstralTextOf(name), AstralTextOfData(data), AstralTextOf(error));
    }));
}

static AstralReply PressNode(const AstralRequest *request)
{
    NSString *identifier = AstralStringOfText(request->first);
    BOOL prefix = request->flag;
    NSNumber *pressed = OnMain(^id {
        return @(PressIdentifiedView(identifier, prefix));
    });
    return Flag(pressed.boolValue);
}

static AstralReply InsertComposerText(const AstralRequest *request)
{
    NSString *text = AstralStringOfText(request->first);
    BOOL replace = request->flag;
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralInsertComposerText(text, replace);
    });
    return Flag(YES);
}

static AstralReply OpenReport(const AstralRequest *request)
{
    NSString *text = AstralStringOfText(request->first);
    NSURL *issue = [NSURL URLWithString:AstralStringOfText(request->second)];
    dispatch_async(dispatch_get_main_queue(), ^{
        UIPasteboard.generalPasteboard.string = text;
        if (issue) [UIApplication.sharedApplication openURL:issue options:@{} completionHandler:nil];
    });
    return Done();
}

static UIViewController *TopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

static AstralReply ExportBackup(void)
{
    NSString *text = AstralTakeString(astral_ios_export_backup()) ?: @"";
    NSString *path = [NSTemporaryDirectory() stringByAppendingPathComponent:@"astral-backup.toml"];
    if (![text writeToFile:path atomically:YES encoding:NSUTF8StringEncoding error:nil]) return Flag(NO);
    dispatch_async(dispatch_get_main_queue(), ^{
        UIActivityViewController *sheet = [[UIActivityViewController alloc] initWithActivityItems:@[ [NSURL fileURLWithPath:path] ]
                                                                            applicationActivities:nil];
        UIViewController *top = TopController();
        sheet.popoverPresentationController.sourceView = top.view;
        [top presentViewController:sheet animated:YES completion:nil];
    });
    return Done();
}

static AstralReply ImportBackup(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralPickFile(@[ @"toml" ], ^(NSData *data, NSString *name) {
            NSString *failure = data ? AstralTakeString(astral_ios_apply_backup(AstralTextOfData(data))) : @"could not read the file";
            AstralShowAlert(failure ? @"Import failed" : @"Settings imported",
                            failure ?: @"Missing marketplace addons are being installed.");
        });
    });
    return Done();
}

static AstralReply EditText(const AstralRequest *request)
{
    uint64_t identifier = request->identifier;
    NSString *title = AstralStringOfText(request->first);
    NSString *value = AstralStringOfText(request->second);
    NSUInteger maximum = request->number > 0 ? (NSUInteger)request->number : 0;
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralEditText(title, value, maximum, ^(NSString *edited) {
            astral_ios_complete_text_edit(identifier, AstralTextOf(edited));
        });
    });
    return Flag(YES);
}

static AstralReply EnvironmentFacts(void)
{
    NSMutableDictionary *facts = AstralEnvironmentFacts();
    facts[@"compatibility"] = AstralPhoenixCompatibility() ?: @"";
    facts[@"phoenix"] = AstralPhoenixUUID() ?: @"";
    NSData *json = [NSJSONSerialization dataWithJSONObject:facts options:0 error:nil];
    return Text(json ? [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding] : nil);
}

static AstralReply DismissSheets(void)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
            if (![scene isKindOfClass:UIWindowScene.class]) continue;
            for (UIWindow *window in ((UIWindowScene *)scene).windows) {
                if (window.isKeyWindow && window.rootViewController.presentedViewController)
                    [window.rootViewController dismissViewControllerAnimated:YES completion:nil];
            }
        }
    });
    return Flag(YES);
}

static AstralReply KeepAlive(BOOL on)
{
    NSNumber *kept = OnMain(^id {
        return @(SetKeepAlive(on));
    });
    return Flag(kept.boolValue);
}

static AstralReply HandleShowAlert(const AstralRequest *request)
{
    AstralShowAlert(AstralStringOfText(request->first), AstralStringOfText(request->second));
    return Done();
}

static AstralReply HandleShowToast(const AstralRequest *request)
{
    AstralShowToast(AstralStringOfText(request->first), AstralStringOfText(request->second));
    return Done();
}

static AstralReply HandleTriggerHaptic(const AstralRequest *request)
{
    return Haptic(AstralStringOfText(request->first));
}

static AstralReply HandleWriteClipboard(const AstralRequest *request)
{
    NSString *text = AstralStringOfText(request->first);
    dispatch_async(dispatch_get_main_queue(), ^{
        UIPasteboard.generalPasteboard.string = text;
    });
    return Flag(YES);
}

static AstralReply HandleReadClipboard(__unused const AstralRequest *request)
{
    return ReadClipboard();
}

static AstralReply HandleOpenUrl(const AstralRequest *request)
{
    return OpenLink(AstralStringOfText(request->first));
}

static AstralReply HandleNotify(const AstralRequest *request)
{
    return Notify(request);
}

static AstralReply HandleSpeak(const AstralRequest *request)
{
    return Speak(request);
}

static AstralReply HandlePlaySound(const AstralRequest *request)
{
    return PlaySound(request);
}

static AstralReply HandleStopAudio(__unused const AstralRequest *request)
{
    return StopAudio();
}

static AstralReply HandleWebAuth(const AstralRequest *request)
{
    return WebAuth(request);
}

static AstralReply HandleCancelWebAuth(const AstralRequest *request)
{
    AstralCancelPluginWebAuth(request->identifier);
    return Done();
}

static AstralReply HandleConfirm(const AstralRequest *request)
{
    return Confirm(request);
}

static AstralReply HandleOpenSheet(const AstralRequest *request)
{
    return OpenSheet(request);
}

static AstralReply HandleUpdateSheet(const AstralRequest *request)
{
    return UpdateSheet(request);
}

static AstralReply HandleCloseSheet(const AstralRequest *request)
{
    AstralCloseSheet(request->identifier);
    return Done();
}

static AstralReply HandlePickFile(const AstralRequest *request)
{
    return PickFile(request);
}

static AstralReply HandleCancelFilePick(const AstralRequest *request)
{
    AstralCancelPluginFile(request->identifier);
    return Done();
}

static AstralReply HandleFetchPickedMedia(const AstralRequest *request)
{
    AstralFetchPickedMedia((NSUInteger)request->identifier);
    return Done();
}

static AstralReply HandlePressNode(const AstralRequest *request)
{
    return PressNode(request);
}

static AstralReply HandleInsertComposerText(const AstralRequest *request)
{
    return InsertComposerText(request);
}

static AstralReply HandleDispatchGateway(const AstralRequest *request)
{
    return Flag(AstralDispatchLocalGatewayEvent(AstralStringOfText(request->first)));
}

static AstralReply HandleRefreshEmoji(__unused const AstralRequest *request)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralRepaintEmojiComposers();
    });
    AstralPhoenixRefreshDecorations();
    return Done();
}

static AstralReply HandleOpenReport(const AstralRequest *request)
{
    return OpenReport(request);
}

static AstralReply HandleExportBackup(__unused const AstralRequest *request)
{
    return ExportBackup();
}

static AstralReply HandleImportBackup(__unused const AstralRequest *request)
{
    return ImportBackup();
}

static AstralReply HandleRestartGatewaySession(__unused const AstralRequest *request)
{
    AstralRestartGatewaySession();
    return Done();
}

static AstralReply HandleEditText(const AstralRequest *request)
{
    return EditText(request);
}

static AstralReply HandleEnvironmentFacts(__unused const AstralRequest *request)
{
    return EnvironmentFacts();
}

static AstralReply HandleDiscordRequest(const AstralRequest *request)
{
    AstralSendDiscordRequest(request->identifier, AstralStringOfText(request->first), AstralStringOfText(request->second),
                             AstralStringOfText(request->third));
    return Done();
}

static AstralReply HandleKeepAlive(const AstralRequest *request)
{
    return KeepAlive(request->flag);
}

static AstralReply HandleOfferPick(const AstralRequest *request)
{
    return Flag(AstralOfferPick(AstralDataOfText(request->second), AstralStringOfText(request->first)));
}

static AstralReply HandleDismissSheets(__unused const AstralRequest *request)
{
    return DismissSheets();
}

static AstralReply HandleRefreshDecorations(__unused const AstralRequest *request)
{
    AstralPhoenixRefreshDecorations();
    dispatch_async(dispatch_get_main_queue(), ^{
        AstralRepaintLuaComposers();
    });
    return Done();
}

static AstralReply HandleRefreshSettings(__unused const AstralRequest *request)
{
    return Done();
}

static NSString *const kAstralIconPrefix = @"Astral";

static NSArray<NSString *> *AstralAppIcons(void)
{
    NSDictionary *icons = [NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleIcons"];
    NSDictionary *alternates = [icons isKindOfClass:NSDictionary.class] ? icons[@"CFBundleAlternateIcons"] : nil;
    if (![alternates isKindOfClass:NSDictionary.class]) return @[];
    NSPredicate *astral = [NSPredicate predicateWithFormat:@"SELF BEGINSWITH %@", kAstralIconPrefix];
    return [[alternates.allKeys filteredArrayUsingPredicate:astral] sortedArrayUsingSelector:@selector(compare:)];
}

static AstralReply HandleAppIcons(__unused const AstralRequest *request)
{
    __block NSString *current = nil;
    __block BOOL supported = NO;
    void (^read)(void) = ^{
        supported = UIApplication.sharedApplication.supportsAlternateIcons;
        current = UIApplication.sharedApplication.alternateIconName;
    };
    if (NSThread.isMainThread) {
        read();
    } else {
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        dispatch_async(dispatch_get_main_queue(), ^{
            read();
            dispatch_semaphore_signal(done);
        });
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC))) return Text(nil);
    }
    NSDictionary *state = @{@"supported" : @(supported), @"current" : current ?: @"", @"icons" : AstralAppIcons()};
    NSData *json = [NSJSONSerialization dataWithJSONObject:state options:0 error:nil];
    return Text(json ? [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding] : nil);
}

static AstralReply HandleSetAppIcon(const AstralRequest *request)
{
    NSString *name = AstralStringOfText(request->first);
    if (name.length && ![AstralAppIcons() containsObject:name]) return Flag(NO);
    dispatch_async(dispatch_get_main_queue(), ^{
        [UIApplication.sharedApplication setAlternateIconName:name.length ? name : nil
                                            completionHandler:^(__unused NSError *error) {
                                                dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                                                    astral_ios_refresh_settings();
                                                });
                                            }];
    });
    return Flag(YES);
}

static AstralReply HandleSyncFolderName(__unused const AstralRequest *request)
{
    return Text(AstralSyncFolderName());
}

static AstralReply HandleSyncChooseFolder(__unused const AstralRequest *request)
{
    AstralChooseSyncFolder();
    return Done();
}

static AstralReply HandleSyncForgetFolder(__unused const AstralRequest *request)
{
    AstralForgetSyncFolder();
    return Done();
}

static AstralReply HandleSyncRead(__unused const AstralRequest *request)
{
    return Text(AstralReadSyncFile());
}

static AstralReply HandleSyncWrite(const AstralRequest *request)
{
    return Flag(AstralWriteSyncFile(AstralStringOfText(request->first)));
}

static AstralReply HandleInjectGateway(const AstralRequest *request)
{
    return Flag(AstralInjectGatewayMessage(AstralStringOfText(request->first)));
}

#define ASTRAL_REQUEST_HANDLER(kind) [AstralRequestKind_##kind] = Handle##kind,
static AstralReply (*const kHandlers[])(const AstralRequest *) = {ASTRAL_REQUEST_KINDS(ASTRAL_REQUEST_HANDLER)};
#undef ASTRAL_REQUEST_HANDLER

static AstralReply Answer(const AstralRequest *request)
{
    if ((size_t)request->kind >= sizeof(kHandlers) / sizeof(kHandlers[0])) return Unsupported();
    return kHandlers[request->kind](request);
}

static AstralReply HandleRequest(const AstralRequest *request)
{
    @autoreleasepool {
        return Answer(request);
    }
}

static void Log(AstralText line)
{
    NSString *text = AstralStringOfText(line);
    NSLog(@"astral %@", text);
    AstralDevAppendLog(text);
}

void AstralAttachBridge(void)
{
    astral_ios_attach((AstralBridge){AstralPhoenixDeliver, HandleRequest, Log, AstralPhoenixLockWire, AstralPhoenixUnlockWire});
}
