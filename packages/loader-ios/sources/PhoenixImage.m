#import "PhoenixImage.h"

#import <dlfcn.h>
#import <mach-o/dyld.h>
#import <string.h>
#import "ImageIdentity.h"

static NSString *const expectedUUID = @"381f8c42-ad92-3e30-80a4-9599f0b11b50";

static BOOL PhoenixClass(Class cls)
{
    return cls && [AstralImageUUIDForPath(class_getImageName(cls)) isEqualToString:expectedUUID];
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
            if (dladdr((const void *)method_getImplementation(method), &owner) &&
                [AstralImageUUIDForPath(owner.dli_fname) isEqualToString:expectedUUID])
                match = method;
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
        if (!name || !strstr(name, "/PhoenixFatClient.framework/PhoenixFatClient")) continue;
        const struct mach_header *header = _dyld_get_image_header(index);
        return [AstralImageUUID(header) isEqualToString:expectedUUID] ? header : NULL;
    }
    return NULL;
}

NSString *AstralPhoenixCompatibility(void)
{
    NSBundle *bundle = NSBundle.mainBundle;
    if (![bundle.bundleIdentifier isEqualToString:@"com.hammerandchisel.discord"] ||
        ![bundle.infoDictionary[@"CFBundleVersion"] isEqualToString:@"124376"])
        return @"unsupported-build";
    Class text = NSClassFromString(@"LibdiscordHost.TextBox");
    if (!text) return @"phoenix-image-unavailable";
    if (!PhoenixClass(text)) return @"unsupported-image";
    if (!AstralPhoenixWindow()) return @"phoenix-root-inactive";
    return @"ready";
}
