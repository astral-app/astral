#import "Marketplace.h"

#import "TomlDocument.h"

#include "LoaderLog.h"
#include "LoaderSettings.h"
#import "AstralState.h"
#import "LuaHost.h"
#import "PluginSignature.h"
#import "Themes.h"

static const NSUInteger kMaxDownload = 512u << 10;

static AstralCatalogState gState = AstralCatalogIdle;
static NSDictionary *gCatalog;
static NSString *gError;
static NSMutableDictionary<NSString *, NSString *> *gInstalls;

NSString *AstralCatalogURL(void)
{
    NSString *fallback =
        [NSURL fileURLWithPath:[AstralResourceDirectory() stringByAppendingPathComponent:@"catalog/index.toml"]].absoluteString;
    std::string url = AstralSettings()->getString("astral", "catalog", fallback.UTF8String);
    return [NSString stringWithUTF8String:url.c_str()] ?: fallback;
}

AstralCatalogState AstralCatalogCurrentState(void)
{
    return gState;
}

NSDictionary *AstralCatalog(void)
{
    return gCatalog;
}

NSString *AstralCatalogError(void)
{
    return gError;
}

NSString *AstralInstallStatus(NSString *packageID)
{
    return gInstalls[packageID];
}

BOOL AstralVersionIsNewer(NSString *candidate, NSString *current)
{
    if (!candidate.length) return NO;
    if (!current.length) return YES;
    return [candidate compare:current options:NSNumericSearch] == NSOrderedDescending;
}

static NSData *Fetch(NSURL *url, NSString **failure)
{
    if (url.isFileURL) {
        NSData *data = [NSData dataWithContentsOfURL:url options:0 error:nil];
        if (!data) *failure = [@"missing " stringByAppendingString:url.lastPathComponent];
        return data.length <= kMaxDownload ? data : nil;
    }
    if (![url.scheme isEqualToString:@"https"]) {
        *failure = @"catalogs must use https";
        return nil;
    }
    __block NSData *result = nil;
    __block NSString *error = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    NSURLRequest *request = [NSURLRequest requestWithURL:url cachePolicy:NSURLRequestReloadIgnoringLocalCacheData timeoutInterval:15];
    [[NSURLSession.sharedSession
        dataTaskWithRequest:request
          completionHandler:^(NSData *data, NSURLResponse *response, NSError *taskError) {
              NSInteger status = [response isKindOfClass:NSHTTPURLResponse.class] ? ((NSHTTPURLResponse *)response).statusCode : 0;
              if (taskError)
                  error = taskError.localizedDescription;
              else if (status != 200)
                  error = [NSString stringWithFormat:@"HTTP %ld", (long)status];
              else if (data.length > kMaxDownload)
                  error = @"download too large";
              else
                  result = data;
              dispatch_semaphore_signal(done);
          }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    if (error) *failure = error;
    return result;
}

static NSURL *Resolve(NSString *relative)
{
    return [NSURL URLWithString:relative relativeToURL:[NSURL URLWithString:AstralCatalogURL()]].absoluteURL;
}

static NSArray *Entries(NSDictionary *catalog, NSString *key)
{
    NSMutableArray *entries = [NSMutableArray array];
    for (NSDictionary *entry in [catalog[key] isKindOfClass:NSArray.class] ? catalog[key] : @[]) {
        if ([entry isKindOfClass:NSDictionary.class] && [entry[@"id"] isKindOfClass:NSString.class] &&
            [entry[@"name"] isKindOfClass:NSString.class] && [entry[@"base"] isKindOfClass:NSString.class])
            [entries addObject:entry];
    }
    return entries;
}

void AstralCatalogRefresh(void (^changed)(void))
{
    if (gState == AstralCatalogLoading) return;
    gState = AstralCatalogLoading;
    changed();
    NSURL *url = [NSURL URLWithString:AstralCatalogURL()];
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSString *failure = nil;
        NSData *data = url ? Fetch(url, &failure) : nil;
        NSURL *signatureURL = url ? [NSURL URLWithString:[url.absoluteString stringByAppendingString:@".sig"]] : nil;
        NSData *signatureData = data ? Fetch(signatureURL, &failure) : nil;
        NSString *signature = signatureData ? [[NSString alloc] initWithData:signatureData encoding:NSUTF8StringEncoding] : nil;
        if (data && !AstralVerifySignature(data, signature)) {
            data = nil;
            failure = @"catalog signature does not match";
        }
        id parsed = AstralParseTOML(data);
        NSDictionary *catalog = nil;
        if ([parsed isKindOfClass:NSDictionary.class]) {
            catalog = @{
                @"loader" : [parsed[@"loader"] isKindOfClass:NSDictionary.class] ? parsed[@"loader"] : @{},
                @"plugins" : Entries(parsed, @"plugins"),
                @"themes" : Entries(parsed, @"themes")
            };
        } else if (!failure) {
            failure = @"catalog is not valid JSON";
        }
        dispatch_async(dispatch_get_main_queue(), ^{
            gCatalog = catalog ?: gCatalog;
            gError = failure;
            gState = catalog ? AstralCatalogReady : AstralCatalogFailed;
            if (failure) loader::log(loader::LogLevel::Error, "marketplace", std::string("Failed to load catalog: ") + failure.UTF8String);
            changed();
        });
    });
}

static NSString *InstallPlugin(NSDictionary *package)
{
    NSString *identifier = package[@"id"];
    NSString *base = [package[@"base"] hasSuffix:@"/"] ? package[@"base"] : [package[@"base"] stringByAppendingString:@"/"];
    NSString *failure = nil;
    NSData *signatureData = Fetch(Resolve([base stringByAppendingString:@"signature"]), &failure);
    NSDictionary<NSString *, NSString *> *signed_ = AstralSignedFiles(signatureData);
    if (!signed_) return failure ?: @"signature does not match";
    NSData *manifestData = Fetch(Resolve([base stringByAppendingString:@"manifest.toml"]), &failure);
    NSDictionary *manifest = AstralParseTOML(manifestData);
    if (![manifest isKindOfClass:NSDictionary.class] || ![manifest[@"id"] isEqual:identifier]) return failure ?: @"manifest does not match";
    NSString *main = [manifest[@"main"] isKindOfClass:NSString.class] ? manifest[@"main"] : @"main.lua";
    if ([main containsString:@"/"] || !signed_[main]) return @"invalid main file";
    NSString *plugins = [AstralStateDirectory() stringByAppendingPathComponent:@"plugins"];
    NSString *staging = [plugins stringByAppendingPathComponent:[@"." stringByAppendingString:identifier]];
    NSString *target = [plugins stringByAppendingPathComponent:identifier];
    NSFileManager *files = NSFileManager.defaultManager;
    [files removeItemAtPath:staging error:nil];
    if (![files createDirectoryAtPath:staging withIntermediateDirectories:YES attributes:nil error:nil] ||
        ![signatureData writeToFile:[staging stringByAppendingPathComponent:@"signature"] atomically:YES]) {
        return @"could not write files";
    }
    for (NSString *path in signed_) {
        NSData *data =
            [path isEqualToString:@"manifest.toml"] ? manifestData : Fetch(Resolve([base stringByAppendingString:path]), &failure);
        NSString *destination = [staging stringByAppendingPathComponent:path];
        [files createDirectoryAtPath:destination.stringByDeletingLastPathComponent
            withIntermediateDirectories:YES
                             attributes:nil
                                  error:nil];
        if (!data || ![data writeToFile:destination atomically:YES]) {
            [files removeItemAtPath:staging error:nil];
            return failure ?: [@"could not download " stringByAppendingString:path];
        }
    }
    if (!AstralVerifyPluginDirectory(staging, main)) {
        [files removeItemAtPath:staging error:nil];
        return @"downloaded files do not match the signature";
    }
    [files removeItemAtPath:target error:nil];
    if (![files moveItemAtPath:staging toPath:target error:nil]) return @"could not move files";
    return nil;
}

static NSString *InstallTheme(NSDictionary *package)
{
    NSString *failure = nil;
    NSData *data = Fetch(Resolve(package[@"base"]), &failure);
    NSDictionary *theme = AstralParseTOML(data);
    if (![theme isKindOfClass:NSDictionary.class] || ![theme[@"id"] isEqual:package[@"id"]]) return failure ?: @"theme does not match";
    NSString *directory = AstralLocalThemesDirectory();
    [NSFileManager.defaultManager createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:nil error:nil];
    NSString *path = [directory stringByAppendingPathComponent:[package[@"id"] stringByAppendingPathExtension:@"toml"]];
    return [data writeToFile:path atomically:YES] ? nil : @"could not write theme";
}

void AstralInstallPackage(NSDictionary *package, NSString *kind, void (^changed)(void))
{
    NSString *identifier = package[@"id"];
    if (!gInstalls) gInstalls = [NSMutableDictionary dictionary];
    if ([gInstalls[identifier] isEqualToString:@"Installing…"]) return;
    gInstalls[identifier] = @"Installing…";
    changed();
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        NSString *failure = [kind isEqualToString:@"theme"] ? InstallTheme(package) : InstallPlugin(package);
        dispatch_async(dispatch_get_main_queue(), ^{
            if (failure) {
                gInstalls[identifier] = @"Failed";
                loader::log(loader::LogLevel::Error, "marketplace",
                            std::string("Failed to install ") + identifier.UTF8String + ": " + failure.UTF8String);
            } else {
                [gInstalls removeObjectForKey:identifier];
                if (![kind isEqualToString:@"theme"]) {
                    AstralLuaRemove(identifier);
                    AstralLuaRescan();
                }
            }
            changed();
        });
    });
}
