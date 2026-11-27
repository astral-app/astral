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

static NSString *FeatureName(NSString *extension)
{
    NSDictionary<NSString *, NSString *> *features = @{
        @"OpenInAstral.appex" : @"Open in astral from Safari",
        @"Share.appex" : @"sharing to Discord from other apps",
        @"NotificationService.appex" : @"notification previews",
        @"AstralExtensions.json" : @"astral's extension list",
    };
    return features[extension] ?: extension.stringByDeletingPathExtension;
}

static NSString *FeatureList(NSArray<NSString *> *missing)
{
    NSMutableArray<NSString *> *names = [NSMutableArray arrayWithCapacity:missing.count];
    for (NSString *extension in missing) [names addObject:FeatureName(extension)];
    if (names.count == 1) return names.firstObject;
    NSString *head = [[names subarrayWithRange:NSMakeRange(0, names.count - 1)] componentsJoinedByString:@", "];
    return [NSString stringWithFormat:@"%@ and %@", head, names.lastObject];
}

static NSString *MissingExtensionsMessage(NSArray<NSString *> *missing)
{
    return [NSString stringWithFormat:@"The app you installed astral with removed %@. Sideloaders do this with a free Apple ID, which can "
                                      @"only sign a few app extensions, or when their \"remove extensions\" option is on.\n\nastral "
                                      @"works without them. To get them back, reinstall the IPA with extensions kept: use a paid "
                                      @"developer account or a signing service that keeps them, or turn that option off.",
                                      FeatureList(missing)];
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

    AstralShowAlert(@"Your installer removed some features", MissingExtensionsMessage(missing));
}
