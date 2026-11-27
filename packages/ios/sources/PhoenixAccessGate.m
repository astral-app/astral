#import "PhoenixAccessGate.h"

#import <mach/mach.h>
#import <mach-o/dyld.h>
#import <dlfcn.h>
#import <stdatomic.h>
#import <libkern/OSCacheControl.h>
#import <string.h>

#import "PhoenixImage.h"
#import "PhoenixLocate.h"

enum { kMaxEvaluators = 2 };
static const char *const kPhoenixImage = "/PhoenixFatClient.framework/PhoenixFatClient";

static NSDictionary<NSString *, id> *gResult;
static atomic_bool gAttempted;

static const uint8_t kReturnTrue[8] = {0x20, 0x00, 0x80, 0x52, 0xC0, 0x03, 0x5F, 0xD6};

static NSDictionary<NSString *, id> *Unlock(const struct mach_header *image)
{
    NSMutableDictionary *result = [@{@"applied" : @NO} mutableCopy];
    AstralImageView view = {(const uint8_t *)image, 0, 0, 0};
    uint64_t evaluators[kMaxEvaluators];
    size_t evaluatorCount = astral_locate_phoenix_access_gate(&view, evaluators, kMaxEvaluators);
    result[@"evaluators"] = @(evaluatorCount);
    if (evaluatorCount != kMaxEvaluators) {
        result[@"skipped"] = @"gate-not-validated";
        return result;
    }
    NSMutableSet<NSNumber *> *pages = [NSMutableSet set];
    NSMutableArray *diags = [NSMutableArray array];
    for (size_t index = 0; index < evaluatorCount; index++) {
        uint64_t entry = (uint64_t)(uintptr_t)image + evaluators[index];
        BOOL already = memcmp((const void *)entry, kReturnTrue, sizeof kReturnTrue) == 0;
        [diags addObject:[@{@"offset" : @(evaluators[index]), @"already" : @(already)} mutableCopy]];
        if (already) continue;
        [pages addObject:@(entry & ~(vm_page_size - 1))];
        [pages addObject:@((entry + sizeof kReturnTrue - 1) & ~(vm_page_size - 1))];
    }
    NSMutableArray<NSNumber *> *writable = [NSMutableArray array];
    for (NSNumber *page in pages) {
        kern_return_t kr =
            vm_protect(mach_task_self(), page.unsignedLongLongValue, vm_page_size, NO, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_COPY);
        if (kr != KERN_SUCCESS) {
            for (NSNumber *prior in writable)
                vm_protect(mach_task_self(), prior.unsignedLongLongValue, vm_page_size, NO, VM_PROT_READ | VM_PROT_EXECUTE);
            result[@"skipped"] = @"patch-refused";
            result[@"protect"] = @(kr);
            result[@"diagnostics"] = diags;
            return result;
        }
        [writable addObject:page];
    }
    for (size_t index = 0; index < evaluatorCount; index++) {
        if ([diags[index][@"already"] boolValue]) continue;
        uint64_t entry = (uint64_t)(uintptr_t)image + evaluators[index];
        memcpy((void *)entry, kReturnTrue, sizeof kReturnTrue);
        diags[index][@"patched"] = @YES;
    }
    BOOL restored = YES;
    for (NSNumber *page in writable)
        restored &=
            vm_protect(mach_task_self(), page.unsignedLongLongValue, vm_page_size, NO, VM_PROT_READ | VM_PROT_EXECUTE) == KERN_SUCCESS;
    for (size_t index = 0; index < evaluatorCount; index++)
        sys_icache_invalidate((void *)((uintptr_t)image + evaluators[index]), sizeof kReturnTrue);
    NSUInteger patched = restored ? evaluatorCount : 0;
    result[@"diagnostics"] = diags;
    result[@"patched"] = @(patched);
    result[@"applied"] = @((BOOL)(patched == evaluatorCount));
    if (patched != evaluatorCount) {
        result[@"skipped"] = @"execute-protection-refused";
    }
    return result;
}

static void ImageLoaded(const struct mach_header *header, intptr_t slide)
{
    (void)slide;
    if (atomic_load(&gAttempted)) return;
    Dl_info info = {0};
    if (!dladdr(header, &info) || !info.dli_fname || !strstr(info.dli_fname, kPhoenixImage)) return;
    if (atomic_exchange(&gAttempted, true)) return;
    gResult = Unlock(header);
    NSLog(@"astral phoenix access: %@", gResult);
}

void AstralArmPhoenixAccessUnlock(void)
{
    gResult = @{@"applied" : @NO, @"skipped" : @"awaiting-phoenix-image"};
    _dyld_register_func_for_add_image(ImageLoaded);
}

NSDictionary<NSString *, id> *AstralPhoenixAccessStatus(void)
{
    return gResult ?: @{};
}

void AstralPhoenixAccessHealthy(void) {}
