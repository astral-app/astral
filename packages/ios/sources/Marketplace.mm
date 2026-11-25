#import "Marketplace.h"

#include "AstralMarketplace.h"
#include "AstralSettings.h"
#import "AstralState.h"
#import "FoundationJSON.h"
#import "LuaHost.h"
#import "Themes.h"

static const NSTimeInterval kFetchTimeout = 15;

static NSString *Native(const std::string &value)
{
    return [NSString stringWithUTF8String:value.c_str()] ?: @"";
}

static int FetchURL(const std::string &address, std::string &body)
{
    NSURL *url = [NSURL URLWithString:Native(address)];
    if (!url) return 0;
    __block NSInteger status = 0;
    __block NSData *result = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    NSURLRequest *request = [NSURLRequest requestWithURL:url
                                             cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                         timeoutInterval:kFetchTimeout];
    [[NSURLSession.sharedSession dataTaskWithRequest:request
                                   completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
                                       if (!error && [response isKindOfClass:NSHTTPURLResponse.class]) {
                                           status = ((NSHTTPURLResponse *)response).statusCode;
                                           result = data;
                                       }
                                       dispatch_semaphore_signal(done);
                                   }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    body.assign(static_cast<const char *>(result.bytes), result.length);
    return static_cast<int>(status);
}

static void Installed(bool theme, const std::string &identifier)
{
    NSString *installed = Native(identifier);
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!theme) {
            AstralLuaRemove(installed);
            AstralLuaRescan();
        } else if ([AstralActiveThemeID() isEqualToString:installed]) {
            AstralApplyTheme(installed);
        }
    });
}

static std::function<void()> OnMain(void (^changed)(void))
{
    void (^copied)(void) = [changed copy];
    return [copied] { dispatch_async(dispatch_get_main_queue(), copied); };
}

void AstralConfigureMarketplace(void)
{
    astral::marketplace::configure(*AstralSettings(), AstralStateDirectory().UTF8String, AstralResourceDirectory().UTF8String,
                                   ASTRAL_SIGNING_KEY, FetchURL, Installed);
}

NSString *AstralCatalogURL(void)
{
    return Native(astral::marketplace::url());
}

AstralCatalogState AstralCatalogCurrentState(void)
{
    std::string state = astral::marketplace::state();
    if (state == "loading") return AstralCatalogLoading;
    if (state == "ready") return AstralCatalogReady;
    if (state == "failed") return AstralCatalogFailed;
    return AstralCatalogIdle;
}

NSDictionary *AstralCatalog(void)
{
    id catalog = AstralObjectFromJSON(astral::marketplace::catalog());
    return [catalog isKindOfClass:NSDictionary.class] && [catalog count] ? catalog : nil;
}

NSString *AstralCatalogError(void)
{
    std::string error = astral::marketplace::error();
    return error.empty() ? nil : Native(error);
}

NSString *AstralInstallStatus(NSString *packageID)
{
    auto status = astral::marketplace::installStatus(packageID.UTF8String ?: "");
    return status ? Native(*status) : nil;
}

BOOL AstralVersionIsNewer(NSString *candidate, NSString *current)
{
    if (!candidate.length) return NO;
    if (!current.length) return YES;
    return [candidate compare:current options:NSNumericSearch] == NSOrderedDescending;
}

void AstralCatalogRefresh(void (^changed)(void))
{
    astral::marketplace::refresh(OnMain(changed));
}

void AstralInstallPackage(NSDictionary *package, NSString *kind, void (^changed)(void))
{
    astral::marketplace::install(AstralJSONFromObject(package), kind.UTF8String ?: "", OnMain(changed));
}
