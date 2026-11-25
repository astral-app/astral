#import "PickedMedia.h"

#import <AVFoundation/AVFoundation.h>
#import <Photos/Photos.h>
#import <os/lock.h>

static const NSUInteger kFetchBatch = 64;
static const CFAbsoluteTime kFreshSeconds = 20;

typedef struct {
    int64_t size;
    double duration;
    PHAssetMediaType type;
} PickedEntry;

static os_unfair_lock gLock = OS_UNFAIR_LOCK_INIT;
static NSData *gEntries;
static NSArray<NSString *> *gIdentifiers;
static NSMutableDictionary<NSString *, NSString *> *gVideoIdentifiers;
static NSMutableDictionary<NSString *, NSURL *> *gVideoFiles;
static NSString *const kPickedVideoHost = @"media.discordapp.net";
static NSString *const kPickedVideoPath = @"/astral-picked/";
static CFAbsoluteTime gFetchedAt;
static BOOL gFetching;

static NSString *TypeName(PHAssetMediaType type)
{
    switch (type) {
    case PHAssetMediaTypeImage:
        return @"image";
    case PHAssetMediaTypeVideo:
        return @"video";
    case PHAssetMediaTypeAudio:
        return @"audio";
    default:
        return @"unknown";
    }
}

static int64_t FileSize(PHAsset *asset)
{
    for (PHAssetResource *resource in [PHAssetResource assetResourcesForAsset:asset]) {
        if (resource.type != PHAssetResourceTypePhoto && resource.type != PHAssetResourceTypeVideo) continue;
        id value = [resource valueForKey:@"fileSize"];
        if ([value isKindOfClass:NSNumber.class]) return [value longLongValue];
    }
    return -1;
}

static BOOL LibraryReadable(void)
{
    PHAuthorizationStatus status = [PHPhotoLibrary authorizationStatusForAccessLevel:PHAccessLevelReadWrite];
    return status == PHAuthorizationStatusAuthorized || status == PHAuthorizationStatusLimited;
}

static NSData *FetchEntries(NSUInteger limit, NSArray<NSString *> **identifiers)
{
    PHFetchOptions *options = [PHFetchOptions new];
    options.sortDescriptors = @[ [NSSortDescriptor sortDescriptorWithKey:@"creationDate" ascending:NO] ];
    options.fetchLimit = limit;
    PHFetchResult<PHAsset *> *assets = [PHAsset fetchAssetsWithOptions:options];
    NSMutableData *entries = [NSMutableData dataWithLength:assets.count * sizeof(PickedEntry)];
    PickedEntry *slots = entries.mutableBytes;
    NSMutableArray<NSString *> *names = [NSMutableArray arrayWithCapacity:assets.count];
    [assets enumerateObjectsUsingBlock:^(PHAsset *asset, NSUInteger index, BOOL *stop) {
        slots[index] = (PickedEntry){FileSize(asset), asset.duration, asset.mediaType};
        [names addObject:asset.localIdentifier];
    }];
    *identifiers = names;
    return entries;
}

static void StartFetch(NSUInteger limit, void (^refresh)(void))
{
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        NSData *entries = nil;
        NSArray<NSString *> *identifiers = @[];
        @autoreleasepool {
            entries = LibraryReadable() ? FetchEntries(limit, &identifiers) : [NSData data];
        }
        os_unfair_lock_lock(&gLock);
        BOOL changed = ![entries isEqualToData:gEntries ?: [NSData data]];
        gEntries = entries;
        gIdentifiers = identifiers;
        gFetchedAt = CFAbsoluteTimeGetCurrent();
        gFetching = NO;
        os_unfair_lock_unlock(&gLock);
        if (changed) refresh();
    });
}

BOOL AstralPickedMediaInfo(NSUInteger slot, int64_t *size, double *duration, NSString **type, void (^refresh)(void))
{
    os_unfair_lock_lock(&gLock);
    NSUInteger count = gEntries.length / sizeof(PickedEntry);
    BOOL known = slot < count;
    PickedEntry entry = known ? ((const PickedEntry *)gEntries.bytes)[slot] : (PickedEntry){-1, 0, PHAssetMediaTypeUnknown};
    BOOL stale = CFAbsoluteTimeGetCurrent() - gFetchedAt > kFreshSeconds || (slot >= count && count % kFetchBatch == 0);
    BOOL fetch = stale && !gFetching;
    if (fetch) gFetching = YES;
    NSUInteger limit = MAX(count, (slot / kFetchBatch + 1) * kFetchBatch);
    os_unfair_lock_unlock(&gLock);
    if (fetch) StartFetch(limit, refresh);
    if (!known) return NO;
    *size = entry.size;
    *duration = entry.duration;
    *type = TypeName(entry.type);
    return YES;
}

static NSString *VideoToken(NSString *identifier)
{
    uint64_t hash = 0xcbf29ce484222325ULL;
    for (const char *byte = identifier.UTF8String; byte && *byte; byte++) hash = (hash ^ (uint8_t)*byte) * 0x100000001b3ULL;
    return [NSString stringWithFormat:@"%016llx", hash];
}

static NSURL *RequestVideoFile(NSString *identifier)
{
    PHAsset *asset = [PHAsset fetchAssetsWithLocalIdentifiers:@[ identifier ] options:nil].firstObject;
    if (!asset || asset.mediaType != PHAssetMediaTypeVideo) return nil;
    PHVideoRequestOptions *options = [PHVideoRequestOptions new];
    options.networkAccessAllowed = NO;
    options.version = PHVideoRequestOptionsVersionCurrent;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block NSURL *file = nil;
    [PHImageManager.defaultManager requestAVAssetForVideo:asset
                                                  options:options
                                            resultHandler:^(AVAsset *video, AVAudioMix *mix, NSDictionary *info) {
                                                NSURL *url = [video isKindOfClass:AVURLAsset.class] ? ((AVURLAsset *)video).URL : nil;
                                                if (url.isFileURL) file = [NSURL fileURLWithPath:url.path];
                                                dispatch_semaphore_signal(done);
                                            }];
    dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
    return file;
}

NSString *AstralPickedVideoURL(NSUInteger slot)
{
    os_unfair_lock_lock(&gLock);
    NSString *identifier = slot < gIdentifiers.count ? gIdentifiers[slot] : nil;
    NSUInteger count = gEntries.length / sizeof(PickedEntry);
    BOOL video = identifier && slot < count && ((const PickedEntry *)gEntries.bytes)[slot].type == PHAssetMediaTypeVideo;
    NSString *token = video ? VideoToken(identifier) : nil;
    if (token) {
        if (!gVideoIdentifiers) gVideoIdentifiers = [NSMutableDictionary dictionary];
        if (gVideoIdentifiers.count > 256) [gVideoIdentifiers removeAllObjects];
        gVideoIdentifiers[token] = identifier;
    }
    os_unfair_lock_unlock(&gLock);
    return token ? [NSString stringWithFormat:@"https://%@%@%@.mp4", kPickedVideoHost, kPickedVideoPath, token] : nil;
}

BOOL AstralIsPickedVideoURL(NSURL *url)
{
    return [url.host isEqualToString:kPickedVideoHost] && [url.path hasPrefix:kPickedVideoPath];
}

NSURL *AstralPickedVideoFile(NSURL *url)
{
    NSString *token = url.lastPathComponent.stringByDeletingPathExtension;
    if (!token) return nil;
    os_unfair_lock_lock(&gLock);
    NSURL *file = gVideoFiles[token];
    NSString *identifier = gVideoIdentifiers[token];
    os_unfair_lock_unlock(&gLock);
    if (file || !identifier) return file;
    @autoreleasepool {
        file = RequestVideoFile(identifier);
    }
    if (!file) return nil;
    os_unfair_lock_lock(&gLock);
    if (!gVideoFiles) gVideoFiles = [NSMutableDictionary dictionary];
    if (gVideoFiles.count > 64) [gVideoFiles removeAllObjects];
    gVideoFiles[token] = file;
    os_unfair_lock_unlock(&gLock);
    return file;
}

NSRange AstralByteRange(NSString *header, NSUInteger length)
{
    NSRange none = NSMakeRange(NSNotFound, 0);
    if (![header hasPrefix:@"bytes="] || length == 0) return none;
    NSArray<NSString *> *bounds = [[header substringFromIndex:6] componentsSeparatedByString:@"-"];
    if (bounds.count != 2) return none;
    unsigned long long start = bounds[0].length ? strtoull(bounds[0].UTF8String, NULL, 10) : 0;
    unsigned long long end = bounds[1].length ? strtoull(bounds[1].UTF8String, NULL, 10) : length - 1;
    if (!bounds[0].length && bounds[1].length) {
        unsigned long long suffix = MIN(end, (unsigned long long)length);
        start = length - suffix;
        end = length - 1;
    }
    if (start >= length || end < start) return none;
    end = MIN(end, (unsigned long long)length - 1);
    return NSMakeRange((NSUInteger)start, (NSUInteger)(end - start + 1));
}
