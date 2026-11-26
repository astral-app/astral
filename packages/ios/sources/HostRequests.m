#import "HostRequests.h"

#import <AVFoundation/AVFoundation.h>
#import <UIKit/UIKit.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/message.h>
#import <objc/runtime.h>

#import "AstralCore.h"
#import "BrowserAuth.h"
#import "ConfirmSheet.h"
#import "EmojiComposer.h"
#import "EnvironmentReport.h"
#import "FilePicker.h"
#import "ImageRenderer.h"
#import "LuaComposer.h"
#import "NetworkFilter.h"
#import "NetworkRequest.h"
#import "NotificationTaps.h"
#import "PhoenixImage.h"
#import "PhoenixSettings.h"
#import "PickOffer.h"
#import "PickedMedia.h"
#import "PluginSheet.h"

static const NSTimeInterval kFetchTimeout = 15;
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

static AstralReply Bytes(NSData *data)
{
    return Reply(AstralReplyKind_Bytes, NO, data);
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

static AstralReply StartNetworkRequest(const AstralRequest *request)
{
    NSDictionary *options = DictionaryOf(request->first);
    if (!options) return Flag(NO);
    AstralStartNetworkRequest(request->identifier, options);
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

static AstralReply PickedMedia(const AstralRequest *request)
{
    int64_t size = -1;
    double duration = 0;
    NSString *type = nil;
    if (!AstralPickedMediaInfo((NSUInteger)request->identifier, &size, &duration, &type, ^{
            AstralPhoenixRefreshDecorations();
        }))
        return Text(nil);
    return Text([NSString stringWithFormat:@"%lld\n%f\n%@", size, duration, type ?: @"unknown"]);
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

static AstralReply RenderImage(const AstralRequest *request)
{
    NSDictionary *spec = DictionaryOf(request->first);
    if (!spec) return Bytes(nil);
    return Bytes(OnMain(^id {
        return AstralRenderImage(spec);
    }));
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

static AstralReply Fetch(NSString *address, NSDictionary *headers)
{
    NSURL *url = [NSURL URLWithString:address];
    uint32_t status = 0;
    NSMutableData *reply = [NSMutableData dataWithLength:sizeof status];
    if (!url) return Bytes(reply);
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url
                                                           cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                                       timeoutInterval:kFetchTimeout];
    for (NSString *name in headers) {
        id value = headers[name];
        if ([name isKindOfClass:NSString.class] && [value isKindOfClass:NSString.class]) [request setValue:value forHTTPHeaderField:name];
    }
    __block NSInteger code = 0;
    __block NSData *body = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    [[NSURLSession.sharedSession dataTaskWithRequest:request
                                   completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
                                       if (!error && [response isKindOfClass:NSHTTPURLResponse.class]) {
                                           code = ((NSHTTPURLResponse *)response).statusCode;
                                           body = data;
                                       }
                                       dispatch_semaphore_signal(done);
                                   }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    status = CFSwapInt32HostToBig((uint32_t)code);
    [reply replaceBytesInRange:NSMakeRange(0, sizeof status) withBytes:&status];
    if (body) [reply appendData:body];
    return Bytes(reply);
}

static AstralReply FetchWithToken(const AstralRequest *request)
{
    NSString *token = AstralStringOfText(request->second);
    return Fetch(AstralStringOfText(request->first), token.length ? @{@"Authorization" : token} : @{});
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

static AstralReply Answer(const AstralRequest *request)
{
    switch (request->kind) {
    case AstralRequestKind_ShowAlert:
        AstralShowAlert(AstralStringOfText(request->first), AstralStringOfText(request->second));
        return Done();
    case AstralRequestKind_ShowToast:
        AstralShowToast(AstralStringOfText(request->first), AstralStringOfText(request->second));
        return Done();
    case AstralRequestKind_TriggerHaptic:
        return Haptic(AstralStringOfText(request->first));
    case AstralRequestKind_WriteClipboard: {
        NSString *text = AstralStringOfText(request->first);
        dispatch_async(dispatch_get_main_queue(), ^{
            UIPasteboard.generalPasteboard.string = text;
        });
        return Flag(YES);
    }
    case AstralRequestKind_ReadClipboard:
        return ReadClipboard();
    case AstralRequestKind_OpenUrl:
        return OpenLink(AstralStringOfText(request->first));
    case AstralRequestKind_Notify:
        return Notify(request);
    case AstralRequestKind_Speak:
        return Speak(request);
    case AstralRequestKind_PlaySound:
        return PlaySound(request);
    case AstralRequestKind_StopAudio:
        return StopAudio();
    case AstralRequestKind_NetworkRequest:
        return StartNetworkRequest(request);
    case AstralRequestKind_CancelNetworkRequest:
        AstralCancelNetworkRequest(request->identifier);
        return Done();
    case AstralRequestKind_WebAuth:
        return WebAuth(request);
    case AstralRequestKind_CancelWebAuth:
        AstralCancelPluginWebAuth(request->identifier);
        return Done();
    case AstralRequestKind_Confirm:
        return Confirm(request);
    case AstralRequestKind_OpenSheet:
        return OpenSheet(request);
    case AstralRequestKind_UpdateSheet:
        return UpdateSheet(request);
    case AstralRequestKind_CloseSheet:
        AstralCloseSheet(request->identifier);
        return Done();
    case AstralRequestKind_PickFile:
        return PickFile(request);
    case AstralRequestKind_CancelFilePick:
        AstralCancelPluginFile(request->identifier);
        return Done();
    case AstralRequestKind_PickedMedia:
        return PickedMedia(request);
    case AstralRequestKind_PickedVideo:
        return Text(AstralPickedVideoURL((NSUInteger)request->identifier));
    case AstralRequestKind_PressNode:
        return PressNode(request);
    case AstralRequestKind_InsertComposerText:
        return InsertComposerText(request);
    case AstralRequestKind_RenderImage:
        return RenderImage(request);
    case AstralRequestKind_DispatchGateway:
        return Flag(AstralDispatchLocalGatewayEvent(AstralStringOfText(request->first)));
    case AstralRequestKind_RefreshEmoji:
        dispatch_async(dispatch_get_main_queue(), ^{
            AstralRepaintEmojiComposers();
        });
        AstralPhoenixRefreshDecorations();
        return Done();
    case AstralRequestKind_OpenReport:
        return OpenReport(request);
    case AstralRequestKind_ExportBackup:
        return ExportBackup();
    case AstralRequestKind_ImportBackup:
        return ImportBackup();
    case AstralRequestKind_RestartGatewaySession:
        AstralRestartGatewaySession();
        return Done();
    case AstralRequestKind_EditText:
        return EditText(request);
    case AstralRequestKind_EnvironmentFacts:
        return EnvironmentFacts();
    case AstralRequestKind_Fetch:
        return FetchWithToken(request);
    case AstralRequestKind_DiscordRequest:
        AstralSendDiscordRequest(request->identifier, AstralStringOfText(request->first), AstralStringOfText(request->second),
                                 AstralStringOfText(request->third));
        return Done();
    case AstralRequestKind_KeepAlive:
        return KeepAlive(request->flag);
    case AstralRequestKind_OfferPick:
        return Flag(AstralOfferPick(AstralDataOfText(request->second), AstralStringOfText(request->first)));
    case AstralRequestKind_DismissSheets:
        return DismissSheets();
    case AstralRequestKind_RefreshDecorations:
        AstralPhoenixRefreshDecorations();
        dispatch_async(dispatch_get_main_queue(), ^{
            AstralRepaintLuaComposers();
        });
        return Done();
    case AstralRequestKind_RefreshSettings:
        return Done();
    case AstralRequestKind_InjectGateway:
        return Flag(AstralInjectGatewayMessage(AstralStringOfText(request->first)));
    }
    return Unsupported();
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
