#import "IncomingLinks.h"

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

#import "AstralCore.h"

typedef BOOL (*OpenURLHandler)(id, SEL, UIApplication *, NSURL *, NSDictionary *);
typedef void (*OpenContextsHandler)(id, SEL, UIScene *, NSSet<UIOpenURLContext *> *);

static NSMutableDictionary<NSString *, NSValue *> *gOriginals;

static BOOL OpenedByAstral(NSURL *url)
{
    return url && astral_ios_open_link(AstralTextOf(url.absoluteString));
}

static IMP OriginalFor(id object, SEL selector)
{
    @synchronized(gOriginals) {
        for (Class cls = object_getClass(object); cls; cls = class_getSuperclass(cls)) {
            NSValue *original = gOriginals[[NSString stringWithFormat:@"%s %s", class_getName(cls), sel_getName(selector)]];
            if (original) return (IMP)original.pointerValue;
        }
    }
    return NULL;
}

static BOOL OpenURL(id self, SEL selector, UIApplication *application, NSURL *url, NSDictionary *options)
{
    if (OpenedByAstral(url)) return YES;
    OpenURLHandler original = (OpenURLHandler)OriginalFor(self, selector);
    return original ? original(self, selector, application, url, options) : NO;
}

static void OpenContexts(id self, SEL selector, UIScene *scene, NSSet<UIOpenURLContext *> *contexts)
{
    NSSet<UIOpenURLContext *> *remaining = [contexts objectsPassingTest:^BOOL(UIOpenURLContext *context, __unused BOOL *stop) {
        return !OpenedByAstral(context.URL);
    }];
    OpenContextsHandler original = (OpenContextsHandler)OriginalFor(self, selector);
    if (original && remaining.count) original(self, selector, scene, remaining);
}

static BOOL DefinesMethod(Class cls, SEL selector)
{
    unsigned int count = 0;
    Method *methods = class_copyMethodList(cls, &count);
    BOOL found = NO;
    for (unsigned int index = 0; index < count && !found; index++) found = method_getName(methods[index]) == selector;
    free(methods);
    return found;
}

static void Route(Class cls, SEL selector, IMP replacement, const char *types)
{
    Class definer = cls;
    while (definer && !DefinesMethod(definer, selector)) definer = class_getSuperclass(definer);
    NSString *key = [NSString stringWithFormat:@"%s %s", class_getName(definer ?: cls), sel_getName(selector)];
    @synchronized(gOriginals) {
        if (gOriginals[key]) return;
        if (definer) {
            IMP original = method_setImplementation(class_getInstanceMethod(definer, selector), replacement);
            gOriginals[key] = [NSValue valueWithPointer:(void *)original];
        } else {
            class_addMethod(cls, selector, replacement, types);
            gOriginals[key] = [NSValue valueWithPointer:NULL];
        }
    }
}

static void RouteDelegate(id delegate)
{
    if (!delegate) return;
    Class cls = object_getClass(delegate);
    if ([delegate conformsToProtocol:@protocol(UISceneDelegate)]) Route(cls, @selector(scene:openURLContexts:), (IMP)OpenContexts, "v@:@@");
    if ([delegate conformsToProtocol:@protocol(UIApplicationDelegate)])
        Route(cls, @selector(application:openURL:options:), (IMP)OpenURL, "B@:@@@");
}

void AstralRouteIncomingLinks(void)
{
    gOriginals = [NSMutableDictionary dictionary];
    [NSNotificationCenter.defaultCenter addObserverForName:UISceneWillConnectNotification
                                                    object:nil
                                                     queue:nil
                                                usingBlock:^(NSNotification *notification) {
                                                    RouteDelegate([(UIScene *)notification.object delegate]);
                                                }];
    [NSNotificationCenter.defaultCenter addObserverForName:UIApplicationDidFinishLaunchingNotification
                                                    object:nil
                                                     queue:nil
                                                usingBlock:^(__unused NSNotification *notification) {
                                                    RouteDelegate(UIApplication.sharedApplication.delegate);
                                                    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes)
                                                        RouteDelegate(scene.delegate);
                                                }];
}
