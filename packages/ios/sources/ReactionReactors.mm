#import "ReactionReactors.h"

#import "DiscordSession.h"

static const NSUInteger kFaces = 5;
static const NSUInteger kMaxEntries = 512;
static const NSTimeInterval kRetryAfter = 3;

static NSMutableDictionary<NSString *, NSDictionary *> *gEntries;
static NSMutableSet<NSString *> *gInFlight;
static dispatch_queue_t gNetwork;
static dispatch_queue_t gState;

static NSString *EncodedEmoji(NSString *emoji)
{
    return [emoji stringByAddingPercentEncodingWithAllowedCharacters:NSCharacterSet.alphanumericCharacterSet];
}

static NSArray<NSDictionary *> *Users(id parsed)
{
    if (![parsed isKindOfClass:NSArray.class]) return nil;
    NSMutableArray *users = [NSMutableArray array];
    for (NSDictionary *user in parsed) {
        if (![user isKindOfClass:NSDictionary.class] || ![user[@"id"] isKindOfClass:NSString.class]) continue;
        NSMutableDictionary *entry = [NSMutableDictionary dictionaryWithObject:user[@"id"] forKey:@"id"];
        for (NSString *field in @[ @"username", @"global_name", @"avatar" ]) {
            if ([user[field] isKindOfClass:NSString.class]) entry[field] = user[field];
        }
        [users addObject:entry];
        if (users.count >= kFaces) break;
    }
    return users;
}

static void Store(NSString *key, NSUInteger count, NSArray *users)
{
    dispatch_sync(gState, ^{
        [gInFlight removeObject:key];
        if (gEntries.count >= kMaxEntries) [gEntries removeAllObjects];
        gEntries[key] = users.count
                            ? @{@"count" : @(count),
                                @"users" : users}
                            : @{@"count" : @(count),
                                @"failed" : @(NSDate.date.timeIntervalSince1970)};
    });
}

static void Request(NSString *key, NSString *channelID, NSString *messageID, NSString *emoji, BOOL burst, NSUInteger count)
{
    NSString *token = AstralDiscordSessionToken();
    if (!token) {
        Store(key, count, nil);
        return;
    }
    NSString *url = [NSString stringWithFormat:@"https://discord.com/api/v9/channels/%@/messages/%@/reactions/%@?limit=%lu&type=%d",
                                               channelID, messageID, EncodedEmoji(emoji), (unsigned long)kFaces, burst ? 1 : 0];
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:url]
                                                           cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                                       timeoutInterval:15];
    [request setValue:token forHTTPHeaderField:@"Authorization"];
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block double retryAfter = 0;
    [[NSURLSession.sharedSession
        dataTaskWithRequest:request
          completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
              NSInteger status = [response isKindOfClass:NSHTTPURLResponse.class] ? ((NSHTTPURLResponse *)response).statusCode : 0;
              id parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
              if (status == 429 && [parsed isKindOfClass:NSDictionary.class]) retryAfter = [parsed[@"retry_after"] doubleValue];
              Store(key, count, !error && status == 200 ? Users(parsed) : nil);
              dispatch_semaphore_signal(done);
          }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    if (retryAfter > 0) [NSThread sleepForTimeInterval:MIN(retryAfter, 10)];
}

NSArray<NSDictionary *> *AstralReactors(NSString *channelID, NSString *messageID, NSString *emoji, BOOL burst, NSUInteger count,
                                        BOOL *fresh)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gEntries = [NSMutableDictionary dictionary];
        gInFlight = [NSMutableSet set];
        gNetwork = dispatch_queue_create("app.astral.reactors", DISPATCH_QUEUE_SERIAL);
        gState = dispatch_queue_create("app.astral.reactors.state", DISPATCH_QUEUE_SERIAL);
    });
    if (!channelID.length || !messageID.length || !emoji.length || !count) return nil;
    NSString *key = [NSString stringWithFormat:@"%@:%@:%@:%d", channelID, messageID, emoji, burst];
    __block NSDictionary *entry;
    __block BOOL fetch = NO;
    dispatch_sync(gState, ^{
        entry = gEntries[key];
        BOOL stale = !entry || [entry[@"count"] unsignedIntegerValue] != count;
        BOOL coolingDown = entry[@"failed"] && NSDate.date.timeIntervalSince1970 - [entry[@"failed"] doubleValue] < kRetryAfter && !stale;
        if ((stale || (entry[@"failed"] && !coolingDown)) && ![gInFlight containsObject:key]) {
            [gInFlight addObject:key];
            fetch = YES;
        }
    });
    if (fresh) *fresh = !fetch && entry[@"users"] && [entry[@"count"] unsignedIntegerValue] == count;
    if (fetch) {
        dispatch_async(gNetwork, ^{
            Request(key, channelID, messageID, emoji, burst, count);
        });
    }
    return entry[@"users"];
}
