#import "PickOffer.h"

#import <PhotosUI/PhotosUI.h>
#import <UIKit/UIKit.h>
#import <objc/message.h>
#import <objc/runtime.h>
#import <os/lock.h>

static const CFAbsoluteTime kOfferSeconds = 20;
static const double kAnswerDelay = 0.2;

static os_unfair_lock gLock = OS_UNFAIR_LOCK_INIT;
static NSURL *gOffered;
static CFAbsoluteTime gOfferedUntil;
static void (*gPresent)(UIViewController *, SEL, UIViewController *, BOOL, void (^)(void));

static NSURL *TakeOffer(void)
{
    os_unfair_lock_lock(&gLock);
    NSURL *offered = CFAbsoluteTimeGetCurrent() < gOfferedUntil ? gOffered : nil;
    gOffered = nil;
    os_unfair_lock_unlock(&gLock);
    return offered;
}

static BOOL AnswerImagePicker(UIImagePickerController *picker, NSURL *file)
{
    UIImage *image = [UIImage imageWithContentsOfFile:file.path];
    id<UIImagePickerControllerDelegate> delegate = (id)picker.delegate;
    if (!image || ![delegate respondsToSelector:@selector(imagePickerController:didFinishPickingMediaWithInfo:)]) return NO;
    NSDictionary *info = @{
        UIImagePickerControllerMediaType : @"public.image",
        UIImagePickerControllerImageURL : file,
        UIImagePickerControllerOriginalImage : image,
        UIImagePickerControllerEditedImage : image,
    };
    [delegate imagePickerController:picker didFinishPickingMediaWithInfo:info];
    return YES;
}

static BOOL AnswerPhotoPicker(PHPickerViewController *picker, NSURL *file)
{
    id<PHPickerViewControllerDelegate> delegate = picker.delegate;
    SEL make = NSSelectorFromString(@"_initWithItemProvider:itemIdentifier:");
    if (!delegate || ![PHPickerResult instancesRespondToSelector:make]) return NO;
    NSItemProvider *provider = [[NSItemProvider alloc] initWithContentsOfURL:file];
    provider.suggestedName = file.lastPathComponent.stringByDeletingPathExtension;
    PHPickerResult *result = ((id (*)(id, SEL, id, id))objc_msgSend)([PHPickerResult alloc], make, provider, NSUUID.UUID.UUIDString);
    if (!result) return NO;
    [delegate picker:picker didFinishPicking:@[ result ]];
    return YES;
}

static void Present(UIViewController *self, SEL selector, UIViewController *presented, BOOL animated, void (^completion)(void))
{
    BOOL picker = [presented isKindOfClass:UIImagePickerController.class] || [presented isKindOfClass:PHPickerViewController.class];
    NSURL *offered = picker ? TakeOffer() : nil;
    if (!offered) {
        gPresent(self, selector, presented, animated, completion);
        return;
    }
    presented.view.alpha = 0;
    gPresent(self, selector, presented, NO, ^{
        if (completion) completion();
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(kAnswerDelay * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
            BOOL answered = [presented isKindOfClass:UIImagePickerController.class]
                                ? AnswerImagePicker((UIImagePickerController *)presented, offered)
                                : AnswerPhotoPicker((PHPickerViewController *)presented, offered);
            if (!answered) presented.view.alpha = 1;
        });
    });
}

static void InstallPresentHook(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Method method = class_getInstanceMethod(UIViewController.class, @selector(presentViewController:animated:completion:));
        gPresent = (void *)method_setImplementation(method, (IMP)Present);
    });
}

BOOL AstralOfferPick(NSData *data, NSString *name)
{
    NSURL *directory = [NSURL fileURLWithPath:[NSTemporaryDirectory() stringByAppendingPathComponent:@"astral-offer"] isDirectory:YES];
    [NSFileManager.defaultManager removeItemAtURL:directory error:nil];
    if (![NSFileManager.defaultManager createDirectoryAtURL:directory withIntermediateDirectories:YES attributes:nil error:nil]) return NO;
    NSURL *file = [directory URLByAppendingPathComponent:name];
    if (![data writeToURL:file atomically:YES]) return NO;
    InstallPresentHook();
    os_unfair_lock_lock(&gLock);
    gOffered = file;
    gOfferedUntil = CFAbsoluteTimeGetCurrent() + kOfferSeconds;
    os_unfair_lock_unlock(&gLock);
    return YES;
}
