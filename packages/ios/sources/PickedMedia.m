#import "PickedMedia.h"

#import <AVFoundation/AVFoundation.h>
#import <Photos/Photos.h>

#import "AstralCore.h"

static const int64_t kSecondsToNanoseconds = 5 * NSEC_PER_SEC;

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

static NSArray<NSDictionary *> *FetchEntries(NSUInteger limit)
{
    PHFetchOptions *options = [PHFetchOptions new];
    options.sortDescriptors = @[ [NSSortDescriptor sortDescriptorWithKey:@"creationDate" ascending:NO] ];
    options.fetchLimit = limit;
    PHFetchResult<PHAsset *> *assets = [PHAsset fetchAssetsWithOptions:options];
    NSMutableArray<NSDictionary *> *entries = [NSMutableArray arrayWithCapacity:assets.count];
    [assets enumerateObjectsUsingBlock:^(PHAsset *asset, NSUInteger index, BOOL *stop) {
        [entries addObject:@{
            @"size" : @(FileSize(asset)),
            @"duration" : @(asset.duration),
            @"type" : TypeName(asset.mediaType),
            @"source" : asset.localIdentifier ?: @"",
        }];
    }];
    return entries;
}

void AstralFetchPickedMedia(NSUInteger limit)
{
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
        @autoreleasepool {
            NSArray *entries = LibraryReadable() ? FetchEntries(limit) : @[];
            NSData *json = [NSJSONSerialization dataWithJSONObject:entries options:0 error:nil] ?: [NSData data];
            astral_ios_picked_media_fetched(AstralTextOfData(json));
        }
    });
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
    dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, kSecondsToNanoseconds));
    return file;
}

BOOL AstralIsPickedVideoURL(NSURL *url)
{
    return url && astral_ios_is_picked_video_url(AstralTextOf(url.absoluteString));
}

NSURL *AstralPickedVideoFile(NSURL *url)
{
    AstralText address = AstralTextOf(url.absoluteString);
    NSString *cached = AstralTakeString(astral_ios_picked_video_file(address));
    if (cached) return [NSURL fileURLWithPath:cached];
    NSString *identifier = AstralTakeString(astral_ios_picked_video_source(address));
    if (!identifier) return nil;
    NSURL *file = nil;
    @autoreleasepool {
        file = RequestVideoFile(identifier);
    }
    if (file) astral_ios_remember_picked_video_file(AstralTextOf(url.absoluteString), AstralTextOf(file.path));
    return file;
}

NSRange AstralByteRange(NSString *header, NSUInteger length)
{
    uint64_t start = 0;
    uint64_t count = 0;
    if (!header || !astral_ios_byte_range(AstralTextOf(header), length, &start, &count)) return NSMakeRange(NSNotFound, 0);
    return NSMakeRange((NSUInteger)start, (NSUInteger)count);
}
