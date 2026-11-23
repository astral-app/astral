#import "AppExtensions.h"

#import "FilePicker.h"

static NSArray<NSString *> *ExpectedExtensions(void)
{
    NSString *manifest = [NSBundle.mainBundle pathForResource:@"AstralExtensions" ofType:@"json"];
    if (manifest) {
        NSData *data = [NSData dataWithContentsOfFile:manifest];
        id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
        NSArray *extensions = [parsed isKindOfClass:NSDictionary.class] ? parsed[@"extensions"] : nil;
        if (![extensions isKindOfClass:NSArray.class]) return @[ @"AstralExtensions.json" ];
        NSMutableArray<NSString *> *validated = [NSMutableArray array];
        for (id item in extensions) {
            if (![item isKindOfClass:NSString.class]) return @[ @"AstralExtensions.json" ];
            NSString *name = item;
            if (![name hasSuffix:@".appex"] || [name containsString:@"/"] || [validated containsObject:name]) {
                return @[ @"AstralExtensions.json" ];
            }
            [validated addObject:name];
        }
        return validated;
    }

    NSArray *urlTypes = NSBundle.mainBundle.infoDictionary[@"CFBundleURLTypes"];
    for (NSDictionary *type in urlTypes) {
        if ([type[@"CFBundleURLSchemes"] containsObject:@"astral"]) return @[ @"OpenInAstral.appex", @"Share.appex" ];
    }
    return @[];
}

NSArray<NSString *> *AstralMissingAppExtensions(void)
{
    NSArray<NSString *> *expected = ExpectedExtensions();
    if (expected.count == 0) return @[];

    NSString *plugins = NSBundle.mainBundle.builtInPlugInsPath;
    NSMutableArray<NSString *> *missing = [NSMutableArray array];
    for (NSString *extension in expected) {
        NSString *path = [plugins stringByAppendingPathComponent:extension];
        if (!plugins || ![NSFileManager.defaultManager fileExistsAtPath:path]) [missing addObject:extension];
    }
    return missing;
}

void AstralReportMissingAppExtensions(void)
{
    NSArray<NSString *> *missing = AstralMissingAppExtensions();
    NSString *bundleID = NSBundle.mainBundle.bundleIdentifier ?: @"unknown";
    NSString *key = [NSString stringWithFormat:@"app.astral.missing-extensions.%@", bundleID];
    NSUserDefaults *defaults = NSUserDefaults.standardUserDefaults;
    if (missing.count == 0) {
        [defaults removeObjectForKey:key];
        return;
    }
    if ([[defaults objectForKey:key] isEqualToArray:missing]) return;
    [defaults setObject:missing forKey:key];

    NSString *names = [missing componentsJoinedByString:@", "];
    NSString *message = [NSString
        stringWithFormat:@"This install is missing: %@. Rebuild or reinstall astral with all Discord app extensions included.", names];
    AstralShowAlert(@"Discord extensions are missing", message);
}
