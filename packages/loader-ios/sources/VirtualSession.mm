#import "VirtualSession.h"

#import <Security/Security.h>

#import "VirtualCamera.h"

static NSString *const kSessionFile = @"session.plist";
static NSString *const kImportFile = @"session-import.plist";

static NSArray<id> *KeychainClasses(void)
{
    return @[ (__bridge id)kSecClassGenericPassword, (__bridge id)kSecClassInternetPassword ];
}

static NSArray<NSDictionary *> *ReadItems(void)
{
    NSMutableArray<NSDictionary *> *items = [NSMutableArray array];
    for (id itemClass in KeychainClasses()) {
        CFTypeRef found = NULL;
        NSDictionary *query = @{
            (__bridge id)kSecClass : itemClass,
            (__bridge id)kSecMatchLimit : (__bridge id)kSecMatchLimitAll,
            (__bridge id)kSecReturnAttributes : @YES,
            (__bridge id)kSecReturnData : @YES,
        };
        if (SecItemCopyMatching((__bridge CFDictionaryRef)query, &found) != errSecSuccess || !found) continue;
        for (NSDictionary *item in (__bridge_transfer NSArray *)found) {
            NSData *data = item[(__bridge id)kSecValueData];
            if (!data) continue;
            NSMutableDictionary *kept = [NSMutableDictionary dictionaryWithObject:itemClass forKey:@"class"];
            kept[@"data"] = data;
            for (id key in
                 @[ (__bridge id)kSecAttrService, (__bridge id)kSecAttrAccount, (__bridge id)kSecAttrServer, (__bridge id)kSecAttrLabel ]) {
                if (item[key]) kept[key] = item[key];
            }
            [items addObject:kept];
        }
    }
    return items;
}

static NSDictionary *IdentityOf(NSDictionary *item)
{
    NSMutableDictionary *identity = [NSMutableDictionary dictionaryWithObject:item[@"class"] forKey:(__bridge id)kSecClass];
    for (id key in @[ (__bridge id)kSecAttrService, (__bridge id)kSecAttrAccount, (__bridge id)kSecAttrServer ]) {
        if (item[key]) identity[key] = item[key];
    }
    return identity;
}

static void WriteItem(NSDictionary *item)
{
    if (![item[@"class"] isKindOfClass:NSString.class] || ![item[@"data"] isKindOfClass:NSData.class]) return;
    NSDictionary *identity = IdentityOf(item);
    SecItemDelete((__bridge CFDictionaryRef)identity);
    NSMutableDictionary *added = [identity mutableCopy];
    added[(__bridge id)kSecValueData] = item[@"data"];
    if (item[(__bridge id)kSecAttrLabel]) added[(__bridge id)kSecAttrLabel] = item[(__bridge id)kSecAttrLabel];
    added[(__bridge id)kSecAttrAccessible] = (__bridge id)kSecAttrAccessibleAfterFirstUnlock;
    OSStatus status = SecItemAdd((__bridge CFDictionaryRef)added, NULL);
    if (status != errSecSuccess) NSLog(@"astral Failed to restore keychain item %@: %d", item[(__bridge id)kSecAttrService], (int)status);
}

static void ImportSession(NSString *path)
{
    NSArray *items = [NSArray arrayWithContentsOfFile:path];
    for (id item in [items isKindOfClass:NSArray.class] ? items : @[]) {
        if ([item isKindOfClass:NSDictionary.class]) WriteItem(item);
    }
    [NSFileManager.defaultManager removeItemAtPath:path error:nil];
}

static void ExportSession(NSString *path)
{
    NSArray<NSDictionary *> *items = ReadItems();
    if (!items.count) return;
    NSData *data = [NSPropertyListSerialization dataWithPropertyList:items format:NSPropertyListBinaryFormat_v1_0 options:0 error:nil];
    [data writeToFile:path options:NSDataWritingAtomic | NSDataWritingFileProtectionCompleteUntilFirstUserAuthentication error:nil];
    [NSFileManager.defaultManager setAttributes:@{NSFilePosixPermissions : @0600} ofItemAtPath:path error:nil];
}

void AstralSyncVirtualSession(NSString *stateDirectory)
{
    if (!AstralIsVirtualDevice()) return;
    NSString *import = [stateDirectory stringByAppendingPathComponent:kImportFile];
    NSString *session = [stateDirectory stringByAppendingPathComponent:kSessionFile];
    if ([NSFileManager.defaultManager fileExistsAtPath:import]) ImportSession(import);
    ExportSession(session);
    [NSNotificationCenter.defaultCenter addObserverForName:@"UIApplicationDidEnterBackgroundNotification"
                                                    object:nil
                                                     queue:nil
                                                usingBlock:^(__unused NSNotification *notification) {
                                                    ExportSession(session);
                                                }];
}
