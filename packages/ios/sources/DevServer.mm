#import "NetworkFilter.h"
#import "DevServer.h"

#import <arpa/inet.h>
#import <netinet/in.h>
#import <sys/socket.h>
#import <unistd.h>

#import "AstralState.h"

#include <string>

#include "AstralSettings.h"
#import "Backup.h"
#import "Themes.h"
#import "LuaHost.h"
#import "TomlDocument.h"

static const uint16_t kPort = 9324;
static const NSUInteger kMaxLog = 500;
static const NSUInteger kMaxRequest = 512u << 10;

static NSMutableArray<NSDictionary *> *gLog;
static NSUInteger gLogSequence;
static NSLock *gLogLock;
static BOOL (^gEnabled)(void);
static void (^gChanged)(void);

void AstralDevAppendLog(NSString *line)
{
    if (!gLogLock) return;
    [gLogLock lock];
    [gLog addObject:@{@"id" : @(++gLogSequence), @"time" : @(NSDate.date.timeIntervalSince1970), @"line" : line}];
    while (gLog.count > kMaxLog) [gLog removeObjectAtIndex:0];
    [gLogLock unlock];
}

static NSData *Response(NSInteger status, id body)
{
    NSData *payload = [NSJSONSerialization dataWithJSONObject:body options:0 error:nil] ?: [NSData data];
    NSString *head =
        [NSString stringWithFormat:@"HTTP/1.1 %ld %@\r\nContent-Type: application/json\r\nContent-Length: %lu\r\nConnection: close\r\n\r\n",
                                   (long)status, status == 200 ? @"OK" : @"Error", (unsigned long)payload.length];
    NSMutableData *data = [[head dataUsingEncoding:NSUTF8StringEncoding] mutableCopy];
    [data appendData:payload];
    return data;
}

static NSDictionary *QueryOf(NSString *path)
{
    NSURLComponents *components = [NSURLComponents componentsWithString:[@"http://device" stringByAppendingString:path]];
    NSMutableDictionary *query = [NSMutableDictionary dictionary];
    for (NSURLQueryItem *item in components.queryItems) query[item.name] = item.value ?: @"";
    query[@"_path"] = components.path ?: @"/";
    return query;
}

static NSString *PluginsDirectory(void)
{
    return [AstralStateDirectory() stringByAppendingPathComponent:@"plugins"];
}

static NSData *PutPlugin(NSDictionary *body)
{
    NSString *manifestText = body[@"manifest"], *source = body[@"main"];
    if (![manifestText isKindOfClass:NSString.class] || ![source isKindOfClass:NSString.class])
        return Response(400, @{@"error" : @"manifest and main are required"});
    NSData *manifestData = [manifestText dataUsingEncoding:NSUTF8StringEncoding];
    NSDictionary *manifest = AstralParseTOML(manifestData);
    NSString *identifier = manifest[@"id"];
    NSString *main = [manifest[@"main"] isKindOfClass:NSString.class] ? manifest[@"main"] : @"main.lua";
    if (![identifier isKindOfClass:NSString.class] || [identifier containsString:@"/"] || [identifier containsString:@".."] ||
        [main containsString:@"/"]) {
        return Response(400, @{@"error" : @"invalid manifest"});
    }
    NSString *directory = [PluginsDirectory() stringByAppendingPathComponent:identifier];
    [NSFileManager.defaultManager createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:nil error:nil];
    [manifestData writeToFile:[directory stringByAppendingPathComponent:@"manifest.toml"] atomically:YES];
    [[source dataUsingEncoding:NSUTF8StringEncoding] writeToFile:[directory stringByAppendingPathComponent:main] atomically:YES];
    NSDictionary *modules = [body[@"modules"] isKindOfClass:NSDictionary.class] ? body[@"modules"] : @{};
    for (NSString *path in modules) {
        NSString *text = modules[path];
        BOOL translation = [path hasPrefix:@"i18n/"] && [path.pathExtension isEqualToString:@"toml"];
        if (![path isKindOfClass:NSString.class] || ![text isKindOfClass:NSString.class] ||
            !([path.pathExtension isEqualToString:@"lua"] || translation) || [path hasPrefix:@"/"] || [path containsString:@".."])
            continue;
        NSString *destination = [directory stringByAppendingPathComponent:path];
        [NSFileManager.defaultManager createDirectoryAtPath:destination.stringByDeletingLastPathComponent
                                withIntermediateDirectories:YES
                                                 attributes:nil
                                                      error:nil];
        [[text dataUsingEncoding:NSUTF8StringEncoding] writeToFile:destination atomically:YES];
    }
    [NSFileManager.defaultManager removeItemAtPath:[directory stringByAppendingPathComponent:@"signature"] error:nil];
    dispatch_sync(dispatch_get_main_queue(), ^{
        AstralLuaReload(identifier);
        if (gChanged) gChanged();
    });
    for (NSDictionary *plugin in AstralLuaPlugins()) {
        if ([plugin[@"id"] isEqualToString:identifier])
            return Response(200, @{@"id" : identifier, @"running" : plugin[@"running"], @"error" : plugin[@"error"]});
    }
    return Response(500, @{@"error" : @"the plugin did not load"});
}

static NSString *const kPairingAlphabet = @"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

NSString *AstralDevPairingCode(void)
{
    static NSString *code;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        std::string stored = AstralSettings()->getString("astral", "dev-pairing-code", "");
        if (stored.size() == 8) {
            code = [NSString stringWithUTF8String:stored.c_str()];
        } else {
            NSMutableString *generated = [NSMutableString stringWithCapacity:8];
            for (int index = 0; index < 8; index++) {
                unichar character = [kPairingAlphabet characterAtIndex:arc4random_uniform((uint32_t)kPairingAlphabet.length)];
                [generated appendFormat:@"%C", character];
            }
            code = [generated copy];
            if (AstralSettings()->set("astral", "dev-pairing-code", std::string(code.UTF8String))) AstralSettings()->save();
        }
        NSString *file = [AstralStateDirectory() stringByAppendingPathComponent:@"dev-pairing-code"];
        [code writeToFile:file atomically:YES encoding:NSUTF8StringEncoding error:nil];
        [NSFileManager.defaultManager setAttributes:@{NSFilePosixPermissions : @0600} ofItemAtPath:file error:nil];
    });
    return code;
}

static BOOL PairingMatches(NSString *offered)
{
    NSData *expected = [AstralDevPairingCode() dataUsingEncoding:NSUTF8StringEncoding];
    NSData *given = [[offered ?: @"" uppercaseString] dataUsingEncoding:NSUTF8StringEncoding];
    if (given.length != expected.length) return NO;
    const uint8_t *left = (const uint8_t *)expected.bytes, *right = (const uint8_t *)given.bytes;
    uint8_t difference = 0;
    for (NSUInteger index = 0; index < expected.length; index++) difference |= left[index] ^ right[index];
    return difference == 0;
}

static NSData *HandleRequest(NSString *method, NSString *path, NSData *bodyData, NSString *pairing)
{
    if (!gEnabled()) return Response(403, @{@"error" : @"turn on developer mode in astral's settings"});
    if (!PairingMatches(pairing))
        return Response(401, @{@"error" : @"pairing code missing or wrong; run astral dev pair <code> with the code under Developer mode"});
    NSDictionary *query = QueryOf(path);
    NSString *route = query[@"_path"];
    id body = bodyData.length ? [NSJSONSerialization JSONObjectWithData:bodyData options:0 error:nil] : nil;
    if ([method isEqualToString:@"GET"] && [route isEqualToString:@"/status"]) {
        return Response(200, @{@"astral" : AstralVersion(), @"plugins" : AstralLuaPlugins(), @"network" : AstralNetworkFilterStats()});
    }
    if ([method isEqualToString:@"GET"] && [route isEqualToString:@"/logs"]) {
        NSUInteger since = (NSUInteger)[query[@"since"] integerValue];
        [gLogLock lock];
        NSMutableArray *lines = [NSMutableArray array];
        for (NSDictionary *entry in gLog) {
            if ([entry[@"id"] unsignedIntegerValue] > since) [lines addObject:entry];
        }
        [gLogLock unlock];
        return Response(200, lines);
    }
    if ([method isEqualToString:@"GET"] && [route isEqualToString:@"/backup"]) {
        __block NSString *text = nil;
        dispatch_sync(dispatch_get_main_queue(), ^{
            text = AstralBackupTOML();
        });
        return Response(200, @{@"backup" : text ?: @""});
    }
    if ([method isEqualToString:@"POST"] && [route isEqualToString:@"/backup"] && [body isKindOfClass:NSDictionary.class] &&
        [body[@"backup"] isKindOfClass:NSString.class]) {
        __block NSString *failure = nil;
        dispatch_sync(dispatch_get_main_queue(), ^{
            failure = AstralApplyBackup([body[@"backup"] dataUsingEncoding:NSUTF8StringEncoding], gChanged
                                                                                                      ?: ^{
                                                                                                         });
        });
        return failure ? Response(400, @{@"error" : failure}) : Response(200, @{@"imported" : @YES});
    }
    if ([method isEqualToString:@"GET"] && [route isEqualToString:@"/themes"]) {
        NSMutableArray *themes = [NSMutableArray array];
        for (NSDictionary *theme in AstralThemes()) [themes addObject:theme[@"id"] ?: @""];
        return Response(200, @{@"active" : AstralActiveThemeID(), @"themes" : themes});
    }
    if ([method isEqualToString:@"POST"] && [route isEqualToString:@"/theme"] && [body isKindOfClass:NSDictionary.class] &&
        [body[@"id"] isKindOfClass:NSString.class]) {
        dispatch_sync(dispatch_get_main_queue(), ^{
            AstralApplyTheme(body[@"id"]);
            if (gChanged) gChanged();
        });
        return Response(200, @{@"active" : AstralActiveThemeID()});
    }
    if ([method isEqualToString:@"POST"] && [route isEqualToString:@"/reload"]) {
        __block NSUInteger plugins = 0;
        dispatch_sync(dispatch_get_main_queue(), ^{
            AstralApplyTheme(AstralActiveThemeID());
            for (NSDictionary *plugin in AstralLuaPlugins()) {
                if (![plugin[@"enabled"] boolValue]) continue;
                AstralLuaReload(plugin[@"id"]);
                plugins++;
            }
            if (gChanged) gChanged();
        });
        return Response(200, @{@"theme" : AstralActiveThemeID(), @"plugins" : @(plugins)});
    }
    if ([method isEqualToString:@"POST"] && [route isEqualToString:@"/unbound-theme"] && bodyData.length) {
        __block NSString *failure = nil;
        dispatch_sync(dispatch_get_main_queue(), ^{
            failure = AstralImportUnboundTheme(bodyData, @"Imported");
            if (gChanged) gChanged();
        });
        return failure ? Response(400, @{@"error" : failure}) : Response(200, @{@"imported" : @YES});
    }
    if ([method isEqualToString:@"PUT"] && [route isEqualToString:@"/plugin"] && [body isKindOfClass:NSDictionary.class])
        return PutPlugin(body);
    if ([method isEqualToString:@"POST"] && [route isEqualToString:@"/eval"] && [body isKindOfClass:NSDictionary.class]) {
        NSString *plugin = body[@"plugin"], *code = body[@"code"];
        if (![plugin isKindOfClass:NSString.class] || ![code isKindOfClass:NSString.class])
            return Response(400, @{@"error" : @"plugin and code are required"});
        return Response(200, @{@"result" : AstralLuaEval(plugin, code)});
    }
    return Response(404, @{@"error" : @"unknown route"});
}

static void Serve(int client)
{
    NSMutableData *request = [NSMutableData data];
    uint8_t buffer[16384];
    NSUInteger expected = NSUIntegerMax, headerEnd = 0;
    NSString *method = @"", *path = @"/", *pairing = nil;
    while (request.length < kMaxRequest) {
        ssize_t received = recv(client, buffer, sizeof(buffer), 0);
        if (received <= 0) break;
        [request appendBytes:buffer length:(NSUInteger)received];
        if (!headerEnd) {
            NSRange end = [request rangeOfData:[@"\r\n\r\n" dataUsingEncoding:NSUTF8StringEncoding]
                                       options:0
                                         range:NSMakeRange(0, request.length)];
            if (end.location == NSNotFound) continue;
            headerEnd = end.location + 4;
            NSString *head = [[NSString alloc] initWithData:[request subdataWithRange:NSMakeRange(0, end.location)]
                                                   encoding:NSUTF8StringEncoding];
            NSArray<NSString *> *lines = [head componentsSeparatedByString:@"\r\n"];
            NSArray<NSString *> *first = [lines.firstObject componentsSeparatedByString:@" "];
            if (first.count >= 2) {
                method = first[0];
                path = first[1];
            }
            NSUInteger length = 0;
            for (NSString *line in lines) {
                if ([line.lowercaseString hasPrefix:@"content-length:"]) length = (NSUInteger)[[line substringFromIndex:15] integerValue];
                if ([line.lowercaseString hasPrefix:@"x-astral-pairing:"])
                    pairing = [[line substringFromIndex:17] stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceCharacterSet];
            }
            expected = headerEnd + length;
        }
        if (request.length >= expected) break;
    }
    NSData *body =
        headerEnd && request.length >= headerEnd ? [request subdataWithRange:NSMakeRange(headerEnd, request.length - headerEnd)] : nil;
    NSData *response =
        request.length >= kMaxRequest ? Response(413, @{@"error" : @"request too large"}) : HandleRequest(method, path, body, pairing);
    send(client, response.bytes, response.length, 0);
    close(client);
}

void AstralStartDevServer(BOOL (^enabled)(void), void (^changed)(void))
{
    static dispatch_once_t once;
    gEnabled = [enabled copy];
    gChanged = [changed copy];
    dispatch_once(&once, ^{
        AstralDevPairingCode();
        gLog = [NSMutableArray array];
        gLogLock = [NSLock new];
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
            int listener = socket(AF_INET, SOCK_STREAM, 0);
            if (listener < 0) return;
            int reuse = 1;
            setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
            struct sockaddr_in address = {};
            address.sin_family = AF_INET;
            address.sin_port = htons(kPort);
            address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
            if (bind(listener, (struct sockaddr *)&address, sizeof(address)) != 0 || listen(listener, 8) != 0) {
                close(listener);
                return;
            }
            while (YES) {
                int client = accept(listener, NULL, NULL);
                if (client < 0) continue;
                dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
                    Serve(client);
                });
            }
        });
    });
}
