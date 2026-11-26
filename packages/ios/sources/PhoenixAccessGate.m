#import "PhoenixAccessGate.h"

#import <mach/mach.h>
#import <mach-o/dyld.h>
#import <dlfcn.h>
#import <stdatomic.h>
#import <libkern/OSCacheControl.h>
#import <string.h>

#import "PhoenixImage.h"
#import "PhoenixLocate.h"

static NSString *const kSuite = @"app.astral.phoenix";
static NSString *const kStrikesKey = @"access_gate_strikes";
static const NSInteger kStrikeLimit = 2;
enum { kMaxSlots = 4, kMaxEvaluators = 4 };
static const char *const kAccessExperiment = "2026-11-phoenix-app-access";
static const char *const kPhoenixImage = "/PhoenixFatClient.framework/PhoenixFatClient";

static NSDictionary<NSString *, id> *gResult;
static atomic_bool gAttempted;

static NSUserDefaults *Store(void)
{
    return [[NSUserDefaults alloc] initWithSuiteName:kSuite];
}

static const uint8_t kReturnTrue[8] = {0x20, 0x00, 0x80, 0x52, 0xC0, 0x03, 0x5F, 0xD6};

static BOOL PatchReturnTrue(uint64_t entry, NSMutableDictionary *diag)
{
    if (memcmp((const void *)entry, kReturnTrue, sizeof kReturnTrue) == 0) {
        diag[@"already"] = @YES;
        return YES;
    }
    vm_address_t page = entry & ~(vm_page_size - 1);
    size_t span = (entry + sizeof kReturnTrue > page + vm_page_size) ? vm_page_size * 2 : vm_page_size;
    kern_return_t kr = vm_protect(mach_task_self(), page, span, NO, VM_PROT_READ | VM_PROT_WRITE | VM_PROT_COPY);
    diag[@"protect"] = @(kr);
    if (kr != KERN_SUCCESS) {
        return NO;
    }
    memcpy((void *)entry, kReturnTrue, sizeof kReturnTrue);
    vm_protect(mach_task_self(), page, span, NO, VM_PROT_READ | VM_PROT_EXECUTE);
    sys_icache_invalidate((void *)entry, sizeof kReturnTrue);
    diag[@"patched"] = @YES;
    return YES;
}

static NSDictionary<NSString *, id> *Unlock(const struct mach_header *image)
{
    NSMutableDictionary *result = [@{@"applied" : @NO} mutableCopy];
    NSInteger strikes = [Store() integerForKey:kStrikesKey];
    if (strikes >= kStrikeLimit) {
        result[@"skipped"] = @"disabled-after-unhealthy-launches";
        return result;
    }
    AstralImageView view = {(const uint8_t *)image, 0, 0, 0};
    uint64_t helper = astral_locate_const_referencing(&view, kAccessExperiment);
    result[@"helper"] = @(helper);
    if (helper == 0) {
        result[@"skipped"] = @"gate-not-located";
        return result;
    }
    uint64_t evaluators[kMaxEvaluators];
    size_t evaluatorCount = astral_locate_callers(&view, helper, evaluators, kMaxEvaluators);
    result[@"evaluators"] = @(evaluatorCount);
    if (evaluatorCount == 0 || evaluatorCount >= kMaxEvaluators) {
        result[@"skipped"] = evaluatorCount == 0 ? @"no-evaluators" : @"too-many-evaluators";
        return result;
    }
    [Store() setInteger:strikes + 1 forKey:kStrikesKey];
    NSMutableArray *diags = [NSMutableArray array];
    NSUInteger patched = 0;
    for (size_t index = 0; index < evaluatorCount; index++) {
        NSMutableDictionary *diag = [NSMutableDictionary dictionary];
        uint64_t entry = (uint64_t)(uintptr_t)image + evaluators[index];
        if (PatchReturnTrue(entry, diag)) patched++;
        [diags addObject:diag];
    }
    result[@"diagnostics"] = diags;
    result[@"patched"] = @(patched);
    result[@"applied"] = @(patched == evaluatorCount);
    if (patched != evaluatorCount) {
        result[@"skipped"] = @"patch-refused";
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

void AstralPhoenixAccessHealthy(void)
{
    if ([Store() integerForKey:kStrikesKey] != 0) {
        [Store() setInteger:0 forKey:kStrikesKey];
    }
}
