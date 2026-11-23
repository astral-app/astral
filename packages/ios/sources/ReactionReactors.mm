#import "ReactionReactors.h"

#include "AstralReactors.h"
#include "AstralSession.h"
#import "DiscordSession.h"
#import "FoundationJSON.h"

static astral::reactors::FetchResult Fetch(const std::string &url, const std::string &token)
{
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:[NSString stringWithUTF8String:url.c_str()]]
                                                           cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                                       timeoutInterval:15];
    [request setValue:[NSString stringWithUTF8String:token.c_str()] forHTTPHeaderField:@"Authorization"];
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block astral::reactors::FetchResult result;
    [[NSURLSession.sharedSession dataTaskWithRequest:request
                                   completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
                                       if (!error && [response isKindOfClass:NSHTTPURLResponse.class]) {
                                           result.status = (int)((NSHTTPURLResponse *)response).statusCode;
                                           if (data.length) result.body.assign((const char *)data.bytes, data.length);
                                       }
                                       dispatch_semaphore_signal(done);
                                   }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    return result;
}

static void Prepare(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        astral::reactors::setFetcher(Fetch);
    });
    if (astral::session::token().empty()) {
        NSString *token = AstralDiscordSessionToken();
        if (token) astral::session::setToken(token.UTF8String);
    }
}

NSArray<NSDictionary *> *AstralReactors(NSString *channelID, NSString *messageID, NSString *emoji, BOOL burst, NSUInteger count,
                                        BOOL *fresh)
{
    Prepare();
    bool current = false;
    auto users =
        astral::reactors::lookup(channelID.UTF8String ?: "", messageID.UTF8String ?: "", emoji.UTF8String ?: "", burst, count, current);
    if (fresh) *fresh = current;
    if (!users) return nil;
    id object = AstralObjectFromJSON(*users);
    return [object isKindOfClass:NSArray.class] ? object : nil;
}
