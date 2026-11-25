#import "PickedMedia.h"

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

static NSData *FetchEntries(NSUInteger limit)
{
    PHFetchOptions *options = [PHFetchOptions new];
    options.sortDescriptors = @[ [NSSortDescriptor sortDescriptorWithKey:@"creationDate" ascending:NO] ];
    options.fetchLimit = limit;
    PHFetchResult<PHAsset *> *assets = [PHAsset fetchAssetsWithOptions:options];
    NSMutableData *entries = [NSMutableData dataWithLength:assets.count * sizeof(PickedEntry)];
    PickedEntry *slots = entries.mutableBytes;
    [assets enumerateObjectsUsingBlock:^(PHAsset *asset, NSUInteger index, BOOL *stop) {
        slots[index] = (PickedEntry){FileSize(asset), asset.duration, asset.mediaType};
    }];
    return entries;
}

static void StartFetch(NSUInteger limit, void (^refresh)(void))
{
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        NSData *entries = nil;
        @autoreleasepool {
            entries = LibraryReadable() ? FetchEntries(limit) : [NSData data];
        }
        os_unfair_lock_lock(&gLock);
        BOOL changed = ![entries isEqualToData:gEntries ?: [NSData data]];
        gEntries = entries;
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
