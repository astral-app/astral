#import "PhoenixImage.h"

#import <dlfcn.h>
#import <mach-o/dyld.h>
#import <string.h>
#import "ImageIdentity.h"

static NSString *const kPhoenixImage = @"/PhoenixFatClient.framework/PhoenixFatClient";

static BOOL PhoenixPath(const char *path)
{
    return path && strstr(path, kPhoenixImage.UTF8String) != NULL;
}

static BOOL PhoenixClass(Class cls)
{
    return cls && PhoenixPath(class_getImageName(cls));
}

UIWindow *AstralPhoenixWindow(void)
{
    Class root = NSClassFromString(@"LibdiscordHost.RootController");
    if (!PhoenixClass(root)) return nil;

    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class] || scene.activationState != UISceneActivationStateForegroundActive) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            if (!window.hidden && window.rootViewController.class == root) return window;
        }
    }
    return nil;
}

Method AstralPhoenixOwnedMethod(Class cls, SEL selector, const char *encoding)
{
    if (!PhoenixClass(cls)) return NULL;
    unsigned int count = 0;
    Method *methods = class_copyMethodList(cls, &count);
    Method match = NULL;
    for (unsigned int index = 0; index < count; index++) {
        Method method = methods[index];
        if (method_getName(method) == selector && strcmp(method_getTypeEncoding(method), encoding) == 0) {
            Dl_info owner = {0};
            if (dladdr((const void *)method_getImplementation(method), &owner) && PhoenixPath(owner.dli_fname)) match = method;
            break;
        }
    }
    free(methods);
    return match;
}

const struct mach_header *AstralPhoenixImage(void)
{
    for (uint32_t index = 0; index < _dyld_image_count(); index++) {
        const char *name = _dyld_get_image_name(index);
        if (PhoenixPath(name)) return _dyld_get_image_header(index);
    }
    return NULL;
}

NSString *AstralPhoenixUUID(void)
{
    return AstralImageUUID(AstralPhoenixImage());
}

BOOL AstralIsDiscordApp(void)
{
    NSBundle *bundle = NSBundle.mainBundle;
    if (![bundle.bundlePath.pathExtension isEqualToString:@"app"]) return NO;
    NSString *phoenix = [bundle.privateFrameworksPath stringByAppendingPathComponent:@"PhoenixFatClient.framework"];
    return [NSFileManager.defaultManager fileExistsAtPath:phoenix];
}

NSString *AstralPhoenixCompatibility(void)
{
    if (!AstralIsDiscordApp()) return @"unsupported-build";
    Class text = NSClassFromString(@"LibdiscordHost.TextBox");
    if (!text) return @"phoenix-image-unavailable";
    if (!PhoenixClass(text)) return @"unsupported-image";
    if (!AstralPhoenixWindow()) return @"phoenix-root-inactive";
    return @"ready";
}
