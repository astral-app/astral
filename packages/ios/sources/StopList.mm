#import "StopList.h"

#import "AstralState.h"
#import "PluginSignature.h"
#import "TomlDocument.h"

static NSString *CachePath(NSString *extension)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:[@"catalog-cache." stringByAppendingString:extension]];
}

void AstralCacheVerifiedCatalog(NSData *catalog, NSString *signature)
{
    [catalog writeToFile:CachePath(@"toml") atomically:YES];
    [[signature dataUsingEncoding:NSUTF8StringEncoding] writeToFile:CachePath(@"toml.sig") atomically:YES];
}

void AstralClearCachedCatalog(void)
{
    [NSFileManager.defaultManager removeItemAtPath:CachePath(@"toml") error:nil];
    [NSFileManager.defaultManager removeItemAtPath:CachePath(@"toml.sig") error:nil];
}

static NSDictionary *VerifiedCatalog(NSString *path, NSString *signaturePath)
{
    NSData *catalog = [NSData dataWithContentsOfFile:path];
    NSData *signatureData = [NSData dataWithContentsOfFile:signaturePath];
    NSString *signature = signatureData ? [[NSString alloc] initWithData:signatureData encoding:NSUTF8StringEncoding] : nil;
    if (!catalog || !signature || !AstralVerifySignature(catalog, signature)) return nil;
    id parsed = AstralParseTOML(catalog);
    return [parsed isKindOfClass:NSDictionary.class] ? parsed : nil;
}

static NSDictionary *StopListCatalog(void)
{
    NSDictionary *cached = VerifiedCatalog(CachePath(@"toml"), CachePath(@"toml.sig"));
    if (cached) return cached;
    NSString *bundled = [AstralResourceDirectory() stringByAppendingPathComponent:@"catalog/index.toml"];
    return VerifiedCatalog(bundled, [bundled stringByAppendingString:@".sig"]);
}

NSString *AstralStopListReason(NSString *phoenixUUID)
{
    if (!phoenixUUID.length) return nil;
    NSDictionary *compatibility = StopListCatalog()[@"compatibility"];
    if (![compatibility isKindOfClass:NSDictionary.class]) return nil;
    NSArray *passive = compatibility[@"passive"];
    if (![passive isKindOfClass:NSArray.class]) return nil;
    for (id entry in passive) {
        NSString *uuid = [entry isKindOfClass:NSDictionary.class] ? entry[@"phoenix"] : entry;
        if (![uuid isKindOfClass:NSString.class] || [uuid caseInsensitiveCompare:phoenixUUID] != NSOrderedSame) continue;
        NSString *reason = [entry isKindOfClass:NSDictionary.class] ? entry[@"reason"] : nil;
        return [reason isKindOfClass:NSString.class] && reason.length ? reason : @"listed as passive in the catalog";
    }
    return nil;
}
