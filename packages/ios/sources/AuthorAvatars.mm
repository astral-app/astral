#import "AuthorAvatars.h"

#import "AstralState.h"
#import "DiscordCache.h"
#import "DiscordSession.h"

static const NSTimeInterval kFreshFor = 7 * 24 * 60 * 60;
static const NSTimeInterval kRetryAfter = 60 * 60;
static NSString *const kUsersURL = @"https://discord.com/api/v9/users/";

static NSMutableDictionary<NSString *, NSDictionary *> *gAuthors;
static NSMutableSet<NSString *> *gPending;
static void (^gChanged)(void);

static NSString *CachePath(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"authors.json"];
}

static void Load(void)
{
    if (gAuthors) return;
    gPending = [NSMutableSet set];
    NSData *data = [NSData dataWithContentsOfFile:CachePath()];
    NSDictionary *stored = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
    gAuthors = [stored isKindOfClass:NSDictionary.class] ? [stored mutableCopy] : [NSMutableDictionary dictionary];
}

static void Save(void)
{
    NSData *data = [NSJSONSerialization dataWithJSONObject:gAuthors options:0 error:nil];
    [data writeToFile:CachePath() atomically:YES];
}

static BOOL Snowflake(NSString *value)
{
    return value.length >= 15 && value.length <= 20 &&
           [value rangeOfCharacterFromSet:NSCharacterSet.decimalDigitCharacterSet.invertedSet].location == NSNotFound;
}

static NSString *DefaultAvatar(NSUInteger index)
{
    return [NSString stringWithFormat:@"https://cdn.discordapp.com/embed/avatars/%lu.png", (unsigned long)(index % 6)];
}

static dispatch_queue_t gNetwork;

static NSString *AvatarURL(NSString *userID, NSString *hash, NSUInteger index)
{
    return hash.length ? [NSString stringWithFormat:@"https://cdn.discordapp.com/avatars/%@/%@.png?size=64", userID, hash]
                       : DefaultAvatar(index);
}

static void Store(NSString *userID, NSDictionary *user, NSTimeInterval fetched)
{
    NSDictionary *previous = gAuthors[userID];
    NSMutableDictionary *entry = [NSMutableDictionary dictionaryWithDictionary:@{@"fetched" : @(fetched)}];
    entry[@"avatar"] = user ? ([user[@"avatar"] isKindOfClass:NSString.class] ? user[@"avatar"] : @"") : (previous[@"avatar"] ?: @"");
    if ([user[@"username"] isKindOfClass:NSString.class]) entry[@"username"] = user[@"username"];
    if ([user[@"global_name"] isKindOfClass:NSString.class]) entry[@"global_name"] = user[@"global_name"];
    gAuthors[userID] = entry;
    Save();
}

static void Request(NSString *userID)
{
    NSString *token = AstralDiscordSessionToken();
    if (!token) {
        dispatch_async(dispatch_get_main_queue(), ^{
            [gPending removeObject:userID];
            Store(userID, nil, NSDate.date.timeIntervalSince1970 - kFreshFor + kRetryAfter);
        });
        return;
    }
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:[NSURL URLWithString:[kUsersURL stringByAppendingString:userID]]
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
              NSDictionary *user = !error && status == 200 && [parsed isKindOfClass:NSDictionary.class] ? parsed : nil;
              if (status == 429 && [parsed isKindOfClass:NSDictionary.class]) retryAfter = [parsed[@"retry_after"] doubleValue];
              BOOL missing = status == 404;
              dispatch_async(dispatch_get_main_queue(), ^{
                  [gPending removeObject:userID];
                  NSTimeInterval now = NSDate.date.timeIntervalSince1970;
                  if (user || missing)
                      Store(userID, user ?: @{}, now);
                  else
                      Store(userID, nil, now - kFreshFor + kRetryAfter);
                  if (user && gChanged) gChanged();
              });
              dispatch_semaphore_signal(done);
          }] resume];
    dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
    if (retryAfter > 0) [NSThread sleepForTimeInterval:MIN(retryAfter, 30)];
}

static void Fetch(NSString *userID)
{
    if ([gPending containsObject:userID]) return;
    [gPending addObject:userID];
    if (!gNetwork) gNetwork = dispatch_queue_create("app.astral.author-avatars", DISPATCH_QUEUE_SERIAL);
    dispatch_async(gNetwork, ^{
        Request(userID);
    });
}

NSString *AstralAuthorAvatarURL(NSString *userID, NSUInteger index)
{
    if (!Snowflake(userID)) return DefaultAvatar(index);
    NSDictionary *local = AstralDiscordUser(userID);
    if (local) return AvatarURL(userID, local[@"avatar"], index);
    Load();
    NSDictionary *cached = gAuthors[userID];
    NSTimeInterval age = NSDate.date.timeIntervalSince1970 - [cached[@"fetched"] doubleValue];
    if (!cached || age > kFreshFor) Fetch(userID);
    if (!cached || ![cached[@"username"] length]) return DefaultAvatar(index);
    return AvatarURL(userID, cached[@"avatar"], index);
}

void AstralAuthorAvatarsOnChange(void (^changed)(void))
{
    gChanged = [changed copy];
}
