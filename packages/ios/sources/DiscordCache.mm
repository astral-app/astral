#import "DiscordCache.h"

#import <sqlite3.h>

static const NSTimeInterval kFreshness = 30;
static const uint64_t kAdministrator = 1ull << 3;

static sqlite3 *gDatabase;
static NSMutableDictionary<NSString *, NSArray *> *gCache;

static NSString *DatabasePath(void)
{
    NSURL *support = [NSFileManager.defaultManager URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject;
    NSString *store = [support.path stringByAppendingPathComponent:@"store"];
    NSString *newest = nil;
    NSDate *newestDate = nil;
    for (NSString *account in [NSFileManager.defaultManager contentsOfDirectoryAtPath:store error:nil]) {
        NSString *path = [[store stringByAppendingPathComponent:account] stringByAppendingPathComponent:@"cache.sqlite"];
        NSDate *modified = [NSFileManager.defaultManager attributesOfItemAtPath:path error:nil][NSFileModificationDate];
        if (modified && (!newestDate || [modified compare:newestDate] == NSOrderedDescending)) {
            newest = path;
            newestDate = modified;
        }
    }
    return newest;
}

static sqlite3 *Database(void)
{
    if (gDatabase) return gDatabase;
    NSString *path = DatabasePath();
    if (!path) return nullptr;
    NSString *uri = [@"file:"
        stringByAppendingString:[path stringByAddingPercentEncodingWithAllowedCharacters:NSCharacterSet.URLPathAllowedCharacterSet]];
    if (sqlite3_open_v2([uri stringByAppendingString:@"?mode=ro"].UTF8String, &gDatabase,
                        SQLITE_OPEN_READONLY | SQLITE_OPEN_URI | SQLITE_OPEN_NOMUTEX, nullptr) != SQLITE_OK) {
        sqlite3_close(gDatabase);
        gDatabase = nullptr;
        return nullptr;
    }
    sqlite3_busy_timeout(gDatabase, 50);
    gCache = [NSMutableDictionary dictionary];
    return gDatabase;
}

static id Cached(NSString *key)
{
    NSArray *entry = gCache[key];
    if (!entry || -[entry[0] timeIntervalSinceNow] > kFreshness) return nil;
    return entry[1];
}

static id Remember(NSString *key, id value)
{
    gCache[key] = @[ NSDate.date, value ?: NSNull.null ];
    return value;
}

static NSString *Column(NSString *sql, NSString *argument, int column)
{
    sqlite3 *database = Database();
    sqlite3_stmt *statement = nullptr;
    if (!database || sqlite3_prepare_v2(database, sql.UTF8String, -1, &statement, nullptr) != SQLITE_OK) return nil;
    sqlite3_bind_text(statement, 1, argument.UTF8String, -1, SQLITE_TRANSIENT);
    NSString *result = nil;
    if (sqlite3_step(statement) == SQLITE_ROW) {
        const void *bytes = sqlite3_column_blob(statement, column);
        int length = sqlite3_column_bytes(statement, column);
        if (bytes && length > 0) result = [[NSString alloc] initWithBytes:bytes length:(NSUInteger)length encoding:NSUTF8StringEncoding];
    }
    sqlite3_finalize(statement);
    return result;
}

static NSDictionary *JSONObject(NSString *text)
{
    if (!text) return nil;
    id value = [NSJSONSerialization JSONObjectWithData:[text dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
    return [value isKindOfClass:NSDictionary.class] ? value : nil;
}

static BOOL Snowflake(NSString *value)
{
    return value.length >= 15 && value.length <= 20 &&
           [value rangeOfCharacterFromSet:NSCharacterSet.decimalDigitCharacterSet.invertedSet].location == NSNotFound;
}

static NSDictionary *RoleColors(NSDictionary *role)
{
    NSDictionary *colors = [role[@"colors"] isKindOfClass:NSDictionary.class] ? role[@"colors"] : @{};
    NSMutableDictionary *stops = [NSMutableDictionary dictionary];
    for (NSString *name in @[ @"primary_color", @"secondary_color", @"tertiary_color" ]) {
        if ([colors[name] isKindOfClass:NSNumber.class]) stops[name] = colors[name];
    }
    if (!stops[@"primary_color"] && [role[@"color"] isKindOfClass:NSNumber.class]) stops[@"primary_color"] = role[@"color"];
    return stops;
}

static NSString *Key(NSString *snowflake)
{
    return [[@"" stringByPaddingToLength:20 - snowflake.length withString:@"0" startingAtIndex:0] stringByAppendingString:snowflake];
}

static NSString *Unpadded(NSString *key)
{
    NSUInteger start = 0;
    while (start + 1 < key.length && [key characterAtIndex:start] == '0') start++;
    return [key substringFromIndex:start];
}

NSDictionary *AstralDiscordGuild(NSString *guildID)
{
    if (!Snowflake(guildID)) return nil;
    NSString *key = [@"guild:" stringByAppendingString:guildID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSDictionary *record = JSONObject(Column(@"SELECT v FROM guilds WHERE k = ?", Key(guildID), 0));
    NSDictionary *properties = record[@"properties"];
    if (![properties isKindOfClass:NSDictionary.class]) return Remember(key, nil);
    NSMutableDictionary *roles = [NSMutableDictionary dictionary];
    for (NSDictionary *role in record[@"roles"]) {
        if (![role isKindOfClass:NSDictionary.class] || ![role[@"id"] isKindOfClass:NSString.class]) continue;
        NSMutableDictionary *entry = [NSMutableDictionary dictionaryWithDictionary:@{
            @"id" : role[@"id"],
            @"name" : [role[@"name"] isKindOfClass:NSString.class] ? role[@"name"] : @"",
            @"permissions" : [role[@"permissions"] isKindOfClass:NSString.class] ? role[@"permissions"] : @"0",
            @"color" : [role[@"color"] isKindOfClass:NSNumber.class] ? role[@"color"] : @0,
            @"position" : [role[@"position"] isKindOfClass:NSNumber.class] ? role[@"position"] : @0,
            @"colors" : RoleColors(role),
        }];
        if ([role[@"icon"] isKindOfClass:NSString.class]) entry[@"icon"] = role[@"icon"];
        if ([role[@"unicode_emoji"] isKindOfClass:NSString.class]) entry[@"unicode_emoji"] = role[@"unicode_emoji"];
        roles[role[@"id"]] = entry;
    }
    NSDictionary *guild = @{
        @"id" : guildID,
        @"name" : [properties[@"name"] isKindOfClass:NSString.class] ? properties[@"name"] : @"",
        @"owner_id" : [properties[@"owner_id"] isKindOfClass:NSString.class] ? properties[@"owner_id"] : @"",
        @"roles" : roles,
    };
    return Remember(key, guild);
}

NSDictionary *AstralDiscordMember(NSString *guildID, NSString *userID)
{
    if (!Snowflake(guildID) || !Snowflake(userID)) return nil;
    NSString *key = [NSString stringWithFormat:@"member:%@:%@", guildID, userID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSDictionary *record =
        JSONObject(Column(@"SELECT v FROM members WHERE k = ?", [NSString stringWithFormat:@"%@:%@", Key(guildID), Key(userID)], 0));
    if (!record) return Remember(key, nil);
    NSMutableArray<NSString *> *roles = [NSMutableArray array];
    for (id role in record[@"roles"])
        if ([role isKindOfClass:NSString.class]) [roles addObject:role];
    return Remember(
        key, @{@"user_id" : userID, @"roles" : roles, @"nick" : [record[@"nick"] isKindOfClass:NSString.class] ? record[@"nick"] : @""});
}

NSString *AstralDiscordChannelGuild(NSString *channelID)
{
    if (!Snowflake(channelID)) return nil;
    NSString *key = [@"channel:" stringByAppendingString:channelID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSString *row = Column(@"SELECT k FROM channels WHERE k LIKE '%:' || ?", Key(channelID), 0);
    NSString *guild = Unpadded([row componentsSeparatedByString:@":"].firstObject ?: @"");
    return Remember(key, Snowflake(guild) ? guild : nil);
}

NSDictionary *AstralDiscordUser(NSString *userID)
{
    if (!Snowflake(userID)) return nil;
    NSString *key = [@"user:" stringByAppendingString:userID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSDictionary *record = JSONObject(Column(@"SELECT v FROM users WHERE k = ?", Key(userID), 0));
    if (!record) return Remember(key, nil);
    NSMutableDictionary *user = [NSMutableDictionary dictionaryWithDictionary:@{@"id" : userID}];
    for (NSString *field in @[ @"avatar", @"username", @"global_name" ]) {
        if ([record[field] isKindOfClass:NSString.class]) user[field] = record[field];
    }
    return Remember(key, user);
}

NSString *AstralDiscordFriendNickname(NSString *userID)
{
    if (!Snowflake(userID)) return nil;
    NSString *key = [@"relationship:" stringByAppendingString:userID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSDictionary *record = JSONObject(Column(@"SELECT v FROM relationships WHERE k = ?", Key(userID), 0));
    NSString *nickname = [record[@"nickname"] isKindOfClass:NSString.class] && [record[@"nickname"] length] ? record[@"nickname"] : nil;
    return Remember(key, nickname);
}

NSString *AstralDiscordMessageChannel(NSString *messageID)
{
    if (!Snowflake(messageID)) return nil;
    NSString *key = [@"message:" stringByAppendingString:messageID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSString *row = Column(@"SELECT k FROM messages WHERE k LIKE '%:' || ?", Key(messageID), 0);
    NSString *channel = Unpadded([row componentsSeparatedByString:@":"].firstObject ?: @"");
    return Remember(key, Snowflake(channel) ? channel : nil);
}

NSDictionary *AstralDiscordMessage(NSString *channelID, NSString *messageID)
{
    if (!Snowflake(channelID) || !Snowflake(messageID)) return nil;
    NSString *key = [NSString stringWithFormat:@"message-record:%@:%@", channelID, messageID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSDictionary *record =
        JSONObject(Column(@"SELECT v FROM messages WHERE k = ?", [NSString stringWithFormat:@"%@:%@", Key(channelID), Key(messageID)], 0));
    if (!record) return Remember(key, nil);
    NSDictionary *author = [record[@"author"] isKindOfClass:NSDictionary.class] ? record[@"author"] : @{};
    NSMutableDictionary *message = [NSMutableDictionary dictionaryWithDictionary:@{@"id" : messageID, @"channel_id" : channelID}];
    if ([record[@"webhook_id"] isKindOfClass:NSString.class]) message[@"webhook_id"] = record[@"webhook_id"];
    NSMutableDictionary *authorRecord = [NSMutableDictionary dictionaryWithDictionary:@{
        @"id" : [author[@"id"] isKindOfClass:NSString.class] ? author[@"id"] : @"",
        @"bot" : @([author[@"bot"] isKindOfClass:NSNumber.class] && [author[@"bot"] boolValue]),
    }];
    for (NSString *field in @[ @"username", @"global_name" ]) {
        if ([author[field] isKindOfClass:NSString.class]) authorRecord[field] = author[field];
    }
    message[@"author"] = authorRecord;
    if ([record[@"type"] isKindOfClass:NSNumber.class]) message[@"type"] = record[@"type"];
    NSMutableArray *mentions = [NSMutableArray array];
    for (NSDictionary *mentioned in [record[@"mentions"] isKindOfClass:NSArray.class] ? record[@"mentions"] : @[]) {
        if (![mentioned isKindOfClass:NSDictionary.class] || ![mentioned[@"id"] isKindOfClass:NSString.class]) continue;
        NSMutableDictionary *user = [NSMutableDictionary dictionaryWithObject:mentioned[@"id"] forKey:@"id"];
        for (NSString *field in @[ @"username", @"global_name", @"avatar" ]) {
            if ([mentioned[field] isKindOfClass:NSString.class]) user[field] = mentioned[field];
        }
        NSDictionary *member = [mentioned[@"member"] isKindOfClass:NSDictionary.class] ? mentioned[@"member"] : nil;
        if ([member[@"nick"] isKindOfClass:NSString.class]) user[@"nick"] = member[@"nick"];
        [mentions addObject:user];
    }
    message[@"mentions"] = mentions;
    NSMutableArray *roles = [NSMutableArray array];
    for (id role in [record[@"mention_roles"] isKindOfClass:NSArray.class] ? record[@"mention_roles"] : @[]) {
        if ([role isKindOfClass:NSString.class]) [roles addObject:role];
    }
    message[@"mention_roles"] = roles;
    NSDictionary *reference = [record[@"message_reference"] isKindOfClass:NSDictionary.class] ? record[@"message_reference"] : nil;
    if ([reference[@"message_id"] isKindOfClass:NSString.class] && [reference[@"channel_id"] isKindOfClass:NSString.class]) {
        message[@"reference"] = @{@"message_id" : reference[@"message_id"], @"channel_id" : reference[@"channel_id"]};
    }
    return Remember(key, message);
}

static NSArray<NSDictionary *> *Overwrites(NSString *guildID, NSString *channelID)
{
    NSString *key = [NSString stringWithFormat:@"overwrites:%@:%@", guildID, channelID];
    id cached = Cached(key);
    if (cached) return cached == NSNull.null ? nil : cached;
    NSString *text =
        Column(@"SELECT v FROM channel_overwrites WHERE k = ?", [NSString stringWithFormat:@"%@:%@", Key(guildID), Key(channelID)], 0);
    id value = text ? [NSJSONSerialization JSONObjectWithData:[text dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil] : nil;
    return Remember(key, [value isKindOfClass:NSArray.class] ? value : nil);
}

static uint64_t Bits(id value)
{
    return [value isKindOfClass:NSString.class] ? strtoull([value UTF8String], nullptr, 10) : 0;
}

static uint64_t ApplyOverwrites(uint64_t permissions, NSArray<NSDictionary *> *overwrites, NSString *guildID, NSDictionary *member,
                                NSString *userID)
{
    uint64_t roleAllow = 0, roleDeny = 0;
    NSSet *roles = [NSSet setWithArray:member[@"roles"] ?: @[]];
    for (NSDictionary *overwrite in overwrites) {
        if (![overwrite isKindOfClass:NSDictionary.class] || [overwrite[@"id"] isEqual:userID]) continue;
        if ([overwrite[@"id"] isEqual:guildID]) {
            permissions = (permissions & ~Bits(overwrite[@"deny"])) | Bits(overwrite[@"allow"]);
        } else if ([roles containsObject:overwrite[@"id"]]) {
            roleAllow |= Bits(overwrite[@"allow"]);
            roleDeny |= Bits(overwrite[@"deny"]);
        }
    }
    permissions = (permissions & ~roleDeny) | roleAllow;
    for (NSDictionary *overwrite in overwrites) {
        if (![overwrite isKindOfClass:NSDictionary.class] || ![overwrite[@"id"] isEqual:userID]) continue;
        permissions = (permissions & ~Bits(overwrite[@"deny"])) | Bits(overwrite[@"allow"]);
    }
    return permissions;
}

NSNumber *AstralDiscordPermissions(NSString *guildID, NSString *userID, NSString *channelID)
{
    NSDictionary *guild = AstralDiscordGuild(guildID);
    NSDictionary *member = AstralDiscordMember(guildID, userID);
    if (!guild) return nil;
    if ([guild[@"owner_id"] isEqualToString:userID]) return @(UINT64_MAX >> 11);
    if (!member) return nil;
    uint64_t permissions = Bits(guild[@"roles"][guildID][@"permissions"]);
    for (NSString *role in member[@"roles"]) permissions |= Bits(guild[@"roles"][role][@"permissions"]);
    if (permissions & kAdministrator) return @(UINT64_MAX >> 11);
    if (Snowflake(channelID)) permissions = ApplyOverwrites(permissions, Overwrites(guildID, channelID), guildID, member, userID);
    return @(permissions);
}
