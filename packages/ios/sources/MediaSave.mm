#import "MediaSave.h"

#import <Photos/Photos.h>
#import <objc/runtime.h>
#import <memory>
#import <mutex>

#import "AstralDiscordCache.h"
#import "AstralLog.h"
#import "AstralLuaHost.h"

static const NSUInteger kRememberedMedia = 8;
static const int64_t kPluginWait = 12;
static const NSTimeInterval kDownloadWait = 20;

static std::mutex gMediaLock;
static NSMutableArray<NSURL *> *gRecentMedia;
static IMP gOriginalAddResource;

static NSSet<NSString *> *MediaExtensions(void)
{
    static NSSet<NSString *> *extensions;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        extensions = [NSSet setWithArray:@[ @"mp4", @"mov", @"webm", @"gif", @"png", @"jpg", @"jpeg", @"webp", @"avif" ]];
    });
    return extensions;
}

void AstralRememberMediaRequest(NSURL *url)
{
    NSString *extension = url.pathExtension.lowercaseString;
    if (!extension.length || ![MediaExtensions() containsObject:extension]) return;
    std::lock_guard<std::mutex> lock(gMediaLock);
    if (!gRecentMedia) gRecentMedia = [NSMutableArray array];
    [gRecentMedia removeObject:url];
    [gRecentMedia addObject:url];
    if (gRecentMedia.count > kRememberedMedia) [gRecentMedia removeObjectAtIndex:0];
}

static NSURL *RecentMedia(NSString *extension)
{
    std::lock_guard<std::mutex> lock(gMediaLock);
    for (NSURL *url in gRecentMedia.reverseObjectEnumerator)
        if ([url.pathExtension.lowercaseString isEqualToString:extension]) return url;
    return nil;
}

static std::string Text(NSString *value)
{
    return value ? std::string(value.UTF8String ?: "") : std::string();
}

static NSString *WithoutQuery(NSURL *url)
{
    NSURLComponents *components = [NSURLComponents componentsWithURL:url resolvingAgainstBaseURL:NO];
    components.query = nil;
    components.fragment = nil;
    return components.string ?: url.absoluteString;
}

static astral::luahost::MediaSave SaveFor(NSURL *source, PHAssetResourceType type)
{
    astral::luahost::MediaSave save{Text(source.absoluteString), type == PHAssetResourceTypeVideo ? "video" : "image", "", ""};
    if (auto message = astral::discord::messageContaining(Text(WithoutQuery(source)))) {
        save.channelID = message->first;
        save.messageID = message->second;
    }
    return save;
}

static NSString *PluginReplacement(const astral::luahost::MediaSave &save)
{
    dispatch_semaphore_t answered = dispatch_semaphore_create(0);
    auto replacement = std::make_shared<std::optional<std::string>>();
    uint64_t request = astral::luahost::resolveMediaSave(save, [replacement, answered](std::optional<std::string> url) {
        *replacement = std::move(url);
        dispatch_semaphore_signal(answered);
    });
    if (dispatch_semaphore_wait(answered, dispatch_time(DISPATCH_TIME_NOW, kPluginWait * NSEC_PER_SEC))) {
        astral::luahost::cancelMediaSave(request);
        return nil;
    }
    return *replacement ? [NSString stringWithUTF8String:(*replacement)->c_str()] : nil;
}

static NSURL *Download(NSURL *url, NSURL *beside)
{
    dispatch_semaphore_t finished = dispatch_semaphore_create(0);
    __block NSData *body;
    NSURLSessionConfiguration *configuration = NSURLSessionConfiguration.ephemeralSessionConfiguration;
    configuration.timeoutIntervalForRequest = kDownloadWait;
    NSURLSession *session = [NSURLSession sessionWithConfiguration:configuration];
    [[session dataTaskWithURL:url
            completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
                NSInteger status = [response isKindOfClass:NSHTTPURLResponse.class] ? ((NSHTTPURLResponse *)response).statusCode : 0;
                if (!error && status >= 200 && status < 300 && data.length) body = data;
                dispatch_semaphore_signal(finished);
            }] resume];
    dispatch_semaphore_wait(finished, dispatch_time(DISPATCH_TIME_NOW, (int64_t)((kDownloadWait + 2) * NSEC_PER_SEC)));
    [session finishTasksAndInvalidate];
    if (!body) return nil;
    NSString *extension = url.pathExtension.length ? url.pathExtension.lowercaseString : @"gif";
    NSURL *file =
        [[beside URLByDeletingLastPathComponent] URLByAppendingPathComponent:[@"discord-media." stringByAppendingString:extension]];
    return [body writeToURL:file atomically:YES] ? file : nil;
}

static NSURL *ReplacementFile(PHAssetResourceType type, NSURL *file)
{
    if ((type != PHAssetResourceTypeVideo && type != PHAssetResourceTypePhoto) || !file.isFileURL) return nil;
    if (!astral::luahost::hasMediaSaveHandlers()) return nil;
    NSURL *source = RecentMedia(file.pathExtension.lowercaseString);
    if (!source) return nil;
    NSString *replacement = PluginReplacement(SaveFor(source, type));
    NSURL *replacementURL = replacement ? [NSURL URLWithString:replacement] : nil;
    if (!replacementURL) return nil;
    NSURL *downloaded = Download(replacementURL, file);
    if (!downloaded)
        astral::log(astral::LogLevel::Error, "media", "Failed to download the replacement for saved media " + Text(replacement));
    return downloaded;
}

static void AddResource(id self, SEL selector, PHAssetResourceType type, NSURL *file, PHAssetResourceCreationOptions *options)
{
    auto original = (void (*)(id, SEL, PHAssetResourceType, NSURL *, PHAssetResourceCreationOptions *))gOriginalAddResource;
    NSURL *replacement = ReplacementFile(type, file);
    if (!replacement) return original(self, selector, type, file, options);
    PHAssetResourceCreationOptions *replaced = options ? [options copy] : [PHAssetResourceCreationOptions new];
    replaced.originalFilename = replacement.lastPathComponent;
    replaced.uniformTypeIdentifier = nil;
    original(self, selector, PHAssetResourceTypePhoto, replacement, replaced);
}

__attribute__((constructor)) static void InstallMediaSave(void)
{
    Method method = class_getInstanceMethod(PHAssetCreationRequest.class, @selector(addResourceWithType:fileURL:options:));
    if (method) gOriginalAddResource = method_setImplementation(method, (IMP)AddResource);
}
