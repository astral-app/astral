#import "VirtualCamera.h"

#import <objc/runtime.h>
#include <sys/sysctl.h>

static NSArray<NSString *> *const kCameraStrings = @[ @"localizedName", @"uniqueID", @"modelID", @"manufacturer" ];

static NSString *SystemString(const char *name)
{
    char value[64] = {0};
    size_t size = sizeof(value) - 1;
    if (sysctlbyname(name, value, &size, NULL, 0) != 0) return @"";
    return [NSString stringWithUTF8String:value] ?: @"";
}

NSString *AstralDeviceModel(void)
{
    return [NSString
        stringWithFormat:@"%@ %@%@", SystemString("hw.machine"), SystemString("hw.model"), AstralIsVirtualDevice() ? @" virtual" : @""];
}

bool AstralIsVirtualDevice(void)
{
    int present = 0;
    size_t size = sizeof(present);
    return sysctlbyname("kern.hv_vmm_present", &present, &size, NULL, 0) == 0 && present == 1;
}

static void NeverNil(Class cls, SEL selector)
{
    Method method = class_getInstanceMethod(cls, selector);
    if (!method) return;
    IMP original = method_getImplementation(method);
    IMP guarded = imp_implementationWithBlock(^NSString *(id device) {
        NSString *value = ((NSString * (*)(id, SEL)) original)(device, selector);
        return value ?: @"";
    });
    if (!class_addMethod(cls, selector, guarded, method_getTypeEncoding(method))) method_setImplementation(method, guarded);
}

static bool InheritsFrom(Class cls, Class ancestor)
{
    for (Class current = cls; current; current = class_getSuperclass(current))
        if (current == ancestor) return true;
    return false;
}

void AstralFixVirtualCamera(void)
{
    if (!AstralIsVirtualDevice()) return;
    Class device = NSClassFromString(@"AVCaptureDevice");
    if (!device) return;
    unsigned int count = 0;
    Class *classes = objc_copyClassList(&count);
    for (unsigned int index = 0; index < count; index++) {
        if (!InheritsFrom(classes[index], device)) continue;
        for (NSString *name in kCameraStrings) NeverNil(classes[index], NSSelectorFromString(name));
    }
    free(classes);
}
