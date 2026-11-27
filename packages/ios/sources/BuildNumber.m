#import "BuildNumber.h"

#import <objc/runtime.h>

static NSString *const OriginalBuildKey = @"AstralDiscordBuild";
static NSString *const BuildKey = @"CFBundleVersion";

typedef NSDictionary *(*InfoGetter)(NSBundle *, SEL);
typedef id (*InfoValueGetter)(NSBundle *, SEL, NSString *);

static InfoGetter gOriginalInfo;
static InfoValueGetter gOriginalValue;
static NSString *gOriginalBuild;
static NSDictionary *gRestoredInfo;

static NSDictionary *RestoredInfo(NSBundle *self, SEL selector)
{
    NSDictionary *info = gOriginalInfo(self, selector);
    if (self != NSBundle.mainBundle) return info;
    @synchronized(OriginalBuildKey) {
        if (!gRestoredInfo) {
            NSMutableDictionary *restored = [info mutableCopy];
            restored[BuildKey] = gOriginalBuild;
            gRestoredInfo = [restored copy];
        }
        return gRestoredInfo;
    }
}

static id RestoredValue(NSBundle *self, SEL selector, NSString *key)
{
    if (self == NSBundle.mainBundle && [key isEqualToString:BuildKey]) return gOriginalBuild;
    return gOriginalValue(self, selector, key);
}

void AstralRestoreDiscordBuild(void)
{
    NSString *original = [NSBundle.mainBundle objectForInfoDictionaryKey:OriginalBuildKey];
    if (![original isKindOfClass:NSString.class] || !original.length) return;
    gOriginalBuild = [original copy];
    Method info = class_getInstanceMethod(NSBundle.class, @selector(infoDictionary));
    Method value = class_getInstanceMethod(NSBundle.class, @selector(objectForInfoDictionaryKey:));
    gOriginalInfo = (InfoGetter)method_setImplementation(info, (IMP)RestoredInfo);
    gOriginalValue = (InfoValueGetter)method_setImplementation(value, (IMP)RestoredValue);
}
