#import "EngineSelection.h"

static NSString *const kSuite = @"discord_fat_client";
static NSString *const kClientKey = @"launch_client";
static NSString *const kPhoenix = @"phoenix";
static NSString *const kUnhealthyKey = @"unhealthy_phoenix_launches";

NSDictionary<NSString *, id> *AstralForcePhoenix(void)
{
    NSUserDefaults *engine = [[NSUserDefaults alloc] initWithSuiteName:kSuite];
    NSString *client = [engine stringForKey:kClientKey];
    NSInteger unhealthy = [engine integerForKey:kUnhealthyKey];
    BOOL changed = NO;
    if (![client isEqualToString:kPhoenix]) {
        [engine setObject:kPhoenix forKey:kClientKey];
        changed = YES;
    }
    if (unhealthy != 0) {
        [engine setInteger:0 forKey:kUnhealthyKey];
        changed = YES;
    }
    return @{
        @"previousClient": client ?: @"",
        @"previousUnhealthyLaunches": @(unhealthy),
        @"changed": @(changed),
    };
}
