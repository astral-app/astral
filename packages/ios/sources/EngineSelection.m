#import "EngineSelection.h"

#import "AstralCore.h"
#import "FilePicker.h"

static NSString *const kSuite = @"discord_fat_client";
static NSString *const kClientKey = @"launch_client";
static NSString *const kPhoenix = @"phoenix";
static NSString *const kUnhealthyKey = @"unhealthy_phoenix_launches";
static NSString *const kAstralSuite = @"app.astral.phoenix";
static NSString *const kForcedKey = @"forced_last_launch";
static NSString *const kKicksKey = @"phoenix_kicks";
static const int64_t kNoticeDelay = 4 * NSEC_PER_SEC;

static void ExplainRefusal(void)
{
    NSString *title = AstralTakeString(astral_ios_engine_refused_title());
    NSString *message = AstralTakeString(astral_ios_engine_refused_message());
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, kNoticeDelay), dispatch_get_main_queue(), ^{
        AstralShowAlert(title, message);
    });
}

NSDictionary<NSString *, id> *AstralForcePhoenix(void)
{
    NSUserDefaults *engine = [[NSUserDefaults alloc] initWithSuiteName:kSuite];
    NSUserDefaults *memory = [[NSUserDefaults alloc] initWithSuiteName:kAstralSuite];
    NSString *client = [engine stringForKey:kClientKey];
    NSInteger unhealthy = [engine integerForKey:kUnhealthyKey];
    AstralEngineDecision decision = astral_ios_engine_decide(AstralTextOf(client ?: @""), [memory boolForKey:kForcedKey],
                                                             (uint32_t)MAX(0, [memory integerForKey:kKicksKey]));
    [memory setBool:decision.forced_last_launch forKey:kForcedKey];
    [memory setInteger:decision.kicks forKey:kKicksKey];
    if (decision.newly_refused) ExplainRefusal();
    BOOL changed = NO;
    if (decision.force_phoenix && ![client isEqualToString:kPhoenix]) {
        [engine setObject:kPhoenix forKey:kClientKey];
        changed = YES;
    }
    if (decision.force_phoenix && unhealthy != 0) {
        [engine setInteger:0 forKey:kUnhealthyKey];
        changed = YES;
    }
    return @{
        @"previousClient" : client ?: @"",
        @"previousUnhealthyLaunches" : @(unhealthy),
        @"changed" : @(changed),
        @"kicks" : @(decision.kicks),
        @"refused" : @((BOOL)(!decision.force_phoenix)),
    };
}
