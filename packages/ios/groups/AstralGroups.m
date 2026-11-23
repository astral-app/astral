#import <Foundation/Foundation.h>
#import <dlfcn.h>
#import <objc/runtime.h>

static NSString *const kDiscordAppGroup = @"group.com.hammerandchisel.discord";
static NSString *gSignedGroup;

typedef CFTypeRef (*SecTaskCreateFromSelfFunction)(CFAllocatorRef);
typedef CFTypeRef (*SecTaskCopyValueForEntitlementFunction)(CFTypeRef, CFStringRef, CFErrorRef *);

static NSString *signedAppGroup(void)
{
    SecTaskCreateFromSelfFunction create = (SecTaskCreateFromSelfFunction)dlsym(RTLD_DEFAULT, "SecTaskCreateFromSelf");
    SecTaskCopyValueForEntitlementFunction copy =
        (SecTaskCopyValueForEntitlementFunction)dlsym(RTLD_DEFAULT, "SecTaskCopyValueForEntitlement");
    if (!create || !copy) return nil;
    CFTypeRef task = create(kCFAllocatorDefault);
    if (!task) return nil;
    CFTypeRef value = copy(task, CFSTR("com.apple.security.application-groups"), NULL);
    CFRelease(task);
    NSArray *groups = (__bridge_transfer NSArray *)value;
    if (![groups isKindOfClass:NSArray.class]) return nil;
    for (id group in groups) {
        if ([group isKindOfClass:NSString.class]) return group;
    }
    return nil;
}

static NSString *remappedAppGroup(NSString *identifier)
{
    return [identifier isEqualToString:kDiscordAppGroup] ? gSignedGroup : identifier;
}

static void swizzle(Class cls, SEL selector, IMP replacement, IMP *original)
{
    Method method = class_getInstanceMethod(cls, selector);
    if (method) *original = method_setImplementation(method, replacement);
}

static IMP gOriginalSuite;

static id remappedSuite(id self, SEL selector, NSString *suite)
{
    return ((id (*)(id, SEL, NSString *))gOriginalSuite)(self, selector, remappedAppGroup(suite));
}

static IMP gOriginalContainer;

static NSURL *remappedContainer(id self, SEL selector, NSString *group)
{
    return ((NSURL * (*)(id, SEL, NSString *)) gOriginalContainer)(self, selector, remappedAppGroup(group));
}

__attribute__((constructor)) static void installGroupRemap(void)
{
    gSignedGroup = signedAppGroup();
    if (!gSignedGroup || [gSignedGroup isEqualToString:kDiscordAppGroup]) return;
    swizzle(NSUserDefaults.class, @selector(initWithSuiteName:), (IMP)remappedSuite, &gOriginalSuite);
    swizzle(NSFileManager.class, @selector(containerURLForSecurityApplicationGroupIdentifier:), (IMP)remappedContainer,
            &gOriginalContainer);
}
