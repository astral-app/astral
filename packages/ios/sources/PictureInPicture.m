#import "PictureInPicture.h"

#import <AVFoundation/AVFoundation.h>
#import <AVKit/AVKit.h>

static const NSTimeInterval kStartRetryInterval = 0.2;
static const NSInteger kStartAttempts = 10;

static AVPictureInPictureController *gController;

static void CollectPlayerLayers(CALayer *layer, NSMutableArray<AVPlayerLayer *> *found)
{
    if ([layer isKindOfClass:AVPlayerLayer.class] && ((AVPlayerLayer *)layer).player && !layer.hidden) {
        [found addObject:(AVPlayerLayer *)layer];
    }
    for (CALayer *sublayer in layer.sublayers) CollectPlayerLayers(sublayer, found);
}

static AVPlayerLayer *LargestPlayerLayer(void)
{
    NSMutableArray<AVPlayerLayer *> *found = [NSMutableArray array];
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            if (!window.hidden) CollectPlayerLayers(window.layer, found);
        }
    }
    AVPlayerLayer *largest = nil;
    for (AVPlayerLayer *layer in found) {
        CGSize size = layer.bounds.size;
        CGSize best = largest.bounds.size;
        if (!largest || size.width * size.height > best.width * best.height) largest = layer;
    }
    return largest;
}

static void StartWhenPossible(AVPictureInPictureController *controller, NSInteger attempts)
{
    if (controller != gController) return;
    if (controller.isPictureInPicturePossible) {
        [controller startPictureInPicture];
        return;
    }
    if (attempts <= 0) {
        NSLog(@"[astral] Failed to start picture in picture: never became possible");
        return;
    }
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kStartRetryInterval * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        StartWhenPossible(controller, attempts - 1);
    });
}

BOOL AstralStartPictureInPicture(__unused CGFloat width, __unused CGFloat height)
{
    if (![AVPictureInPictureController isPictureInPictureSupported]) return NO;
    __block BOOL started = NO;
    void (^start)(void) = ^{
        AVPlayerLayer *layer = LargestPlayerLayer();
        if (!layer) return;
        NSError *error = nil;
        AVAudioSession *session = AVAudioSession.sharedInstance;
        if (![session setCategory:AVAudioSessionCategoryPlayback mode:AVAudioSessionModeMoviePlayback options:0 error:&error]) {
            NSLog(@"[astral] Failed to set the playback audio session for picture in picture: %@", error);
        }
        [session setActive:YES error:nil];
        if (gController.isPictureInPictureActive) [gController stopPictureInPicture];
        gController = [[AVPictureInPictureController alloc] initWithPlayerLayer:layer];
        StartWhenPossible(gController, kStartAttempts);
        started = YES;
    };
    if (NSThread.isMainThread) {
        start();
    } else {
        dispatch_sync(dispatch_get_main_queue(), start);
    }
    return started;
}
