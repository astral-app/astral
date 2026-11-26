#import "MediaSave.h"

#import <Photos/Photos.h>
#import <objc/runtime.h>

#import "AstralCore.h"

static const NSUInteger kRememberedMedia = 8;
static const NSTimeInterval kDownloadWait = 20;
static const NSTimeInterval kDownloadGrace = 2;

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
    @synchronized(MediaExtensions()) {
        if (!gRecentMedia) gRecentMedia = [NSMutableArray array];
        [gRecentMedia removeObject:url];
        [gRecentMedia addObject:url];
        if (gRecentMedia.count > kRememberedMedia) [gRecentMedia removeObjectAtIndex:0];
    }
}

static NSURL *RecentMedia(NSString *extension)
{
    @synchronized(MediaExtensions()) {
        for (NSURL *url in gRecentMedia.reverseObjectEnumerator)
            if ([url.pathExtension.lowercaseString isEqualToString:extension]) return url;
        return nil;
    }
}

static NSString *PluginReplacement(NSURL *source, PHAssetResourceType type)
{
    AstralMediaKind kind = type == PHAssetResourceTypeVideo ? AstralMediaKind_Video : AstralMediaKind_Image;
    return AstralTakeString(astral_ios_replace_saved_media(AstralTextOf(source.absoluteString), kind));
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
    dispatch_semaphore_wait(finished, dispatch_time(DISPATCH_TIME_NOW, (int64_t)((kDownloadWait + kDownloadGrace) * NSEC_PER_SEC)));
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
    if (!astral_ios_has_media_save_handlers()) return nil;
    NSURL *source = RecentMedia(file.pathExtension.lowercaseString);
    if (!source) return nil;
    NSString *replacement = PluginReplacement(source, type);
    NSURL *replacementURL = replacement ? [NSURL URLWithString:replacement] : nil;
    if (!replacementURL) return nil;
    NSURL *downloaded = Download(replacementURL, file);
    if (!downloaded) AstralDevAppendLog([@"Failed to download the replacement for saved media " stringByAppendingString:replacement]);
    return downloaded;
}

typedef void (*AddResourceFunction)(id, SEL, PHAssetResourceType, NSURL *, PHAssetResourceCreationOptions *);

static void AddResource(id self, SEL selector, PHAssetResourceType type, NSURL *file, PHAssetResourceCreationOptions *options)
{
    AddResourceFunction original = (AddResourceFunction)gOriginalAddResource;
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
