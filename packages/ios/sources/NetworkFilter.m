#import "NetworkFilter.h"
#import "MediaSave.h"
#import "PickedMedia.h"

#import <objc/runtime.h>

#import "AstralCore.h"

static NSString *const kHandledKey = @"app.astral.network-filter";
static const NSUInteger kMaxMessageBody = 1u << 20;
enum { kStreamChunk = 16384 };
static const NSInteger kNoContent = 204;
static const NSInteger kOk = 200;
static const NSInteger kPartialContent = 206;

static BOOL (^gStaffDevTools)(void);
static NSURLRequest *gDiscordTemplate;
static NSString *const kApiPrefix = @"/api/v";
static NSArray<NSString *> *DroppedTemplateHeaders(void)
{
    return @[
        @"content-length", @"content-type", @"content-encoding", @"x-context-properties", @"x-audit-log-reason", @"if-none-match",
        @"if-modified-since", @"range", @"idempotency-key"
    ];
}

static BOOL StaffDevTools(void)
{
    return gStaffDevTools && gStaffDevTools();
}

static NSString *HttpMethod(NSURLRequest *request)
{
    return request.HTTPMethod ?: @"GET";
}

static NSString *Path(NSURL *url)
{
    return url.path.length ? url.path : @"/";
}

static BOOL IsJSONBody(NSURLRequest *request)
{
    if (!request.HTTPBody && !request.HTTPBodyStream) return NO;
    if (request.HTTPBody.length > kMaxMessageBody) return NO;
    NSString *contentType = [[request valueForHTTPHeaderField:@"Content-Type"] lowercaseString];
    return [contentType containsString:@"application/json"];
}

static NSData *JSONData(id object)
{
    return [NSJSONSerialization dataWithJSONObject:object options:0 error:nil] ?: [NSData data];
}

static AstralRequestPlan WithProbe(NSURLRequest *request, BOOL note)
{
    NSURL *url = request.URL;
    NSData *headers = JSONData(request.allHTTPHeaderFields ?: @{});
    AstralRequestProbe probe = {
        AstralTextOf(HttpMethod(request)), AstralTextOf(url.absoluteString), AstralTextOf(url.host),
        AstralTextOf(Path(url)),           AstralTextOfData(headers),        IsJSONBody(request),
    };
    if (note) astral_ios_note_request(&probe);
    return astral_ios_plan_request(&probe);
}

static AstralRequestPlan PlanFor(NSURLRequest *request)
{
    return WithProbe(request, NO);
}

static NSData *StreamedBody(NSInputStream *stream)
{
    NSMutableData *body = [NSMutableData data];
    uint8_t buffer[kStreamChunk];
    [stream open];
    while (body.length <= kMaxMessageBody) {
        NSInteger read = [stream read:buffer maxLength:sizeof(buffer)];
        if (read <= 0) break;
        [body appendBytes:buffer length:(NSUInteger)read];
    }
    [stream close];
    return body;
}

static void MaterializeBody(NSMutableURLRequest *request)
{
    if (request.HTTPBody || !request.HTTPBodyStream) return;
    request.HTTPBody = StreamedBody(request.HTTPBodyStream);
}

static NSArray *HeaderPairs(NSDictionary *fields)
{
    NSMutableArray *pairs = [NSMutableArray array];
    for (NSString *name in fields) [pairs addObject:@[ name.lowercaseString, [fields[name] description] ]];
    return pairs;
}

static NSDictionary *PlainRequest(NSURLRequest *request)
{
    return @{
        @"method" : HttpMethod(request),
        @"url" : request.URL.absoluteString ?: @"",
        @"path" : Path(request.URL),
        @"headers" : HeaderPairs(request.allHTTPHeaderFields ?: @{}),
        @"body_base64" : [request.HTTPBody base64EncodedStringWithOptions:0] ?: @"",
    };
}

static NSData *BodyOf(NSDictionary *plain)
{
    NSString *encoded = [plain[@"body_base64"] isKindOfClass:NSString.class] ? plain[@"body_base64"] : @"";
    return [[NSData alloc] initWithBase64EncodedString:encoded options:0] ?: [NSData data];
}

static NSDictionary<NSString *, NSString *> *HeadersOf(NSDictionary *plain)
{
    NSMutableDictionary<NSString *, NSString *> *headers = [NSMutableDictionary dictionary];
    for (NSArray *pair in [plain[@"headers"] isKindOfClass:NSArray.class] ? plain[@"headers"] : @[]) {
        if (pair.count == 2 && [pair[0] isKindOfClass:NSString.class] && [pair[1] isKindOfClass:NSString.class]) headers[pair[0]] = pair[1];
    }
    return headers;
}

static void ApplyPlainRequest(NSMutableURLRequest *request, NSDictionary *plain)
{
    NSURL *url = [plain[@"url"] isKindOfClass:NSString.class] ? [NSURL URLWithString:plain[@"url"]] : nil;
    if (url) request.URL = url;
    if ([plain[@"method"] isKindOfClass:NSString.class]) request.HTTPMethod = plain[@"method"];
    NSMutableDictionary<NSString *, NSString *> *headers = [HeadersOf(plain) mutableCopy];
    [headers removeObjectForKey:@"content-length"];
    request.allHTTPHeaderFields = headers;
    request.HTTPBodyStream = nil;
    request.HTTPBody = BodyOf(plain);
}

static NSHTTPURLResponse *ResponseOf(NSURL *url, NSDictionary *plain, NSUInteger length)
{
    NSMutableDictionary<NSString *, NSString *> *headers = [HeadersOf(plain) mutableCopy];
    [headers removeObjectForKey:@"content-encoding"];
    headers[@"Content-Length"] = [NSString stringWithFormat:@"%lu", (unsigned long)length];
    NSInteger status = [plain[@"status"] isKindOfClass:NSNumber.class] ? [plain[@"status"] integerValue] : kOk;
    return [[NSHTTPURLResponse alloc] initWithURL:url statusCode:status HTTPVersion:@"HTTP/1.1" headerFields:headers];
}

static NSDictionary *PlainResponse(NSHTTPURLResponse *response, NSData *body)
{
    return @{
        @"status" : @(response.statusCode),
        @"headers" : HeaderPairs(response.allHeaderFields ?: @{}),
        @"body_base64" : [body base64EncodedStringWithOptions:0] ?: @"",
    };
}

static NSString *ApiBase(NSURL *url)
{
    NSString *path = url.path ?: @"";
    if (![url.host.lowercaseString isEqualToString:@"discord.com"] || ![path hasPrefix:kApiPrefix]) return nil;
    NSRange slash = [path rangeOfString:@"/" options:0 range:NSMakeRange(kApiPrefix.length, path.length - kApiPrefix.length)];
    NSString *prefix = slash.location == NSNotFound ? path : [path substringToIndex:slash.location];
    return [NSString stringWithFormat:@"https://%@%@", url.host, prefix];
}

static void RememberDiscordTemplate(NSURLRequest *request)
{
    if (!ApiBase(request.URL) || ![request valueForHTTPHeaderField:@"Authorization"].length) return;
    NSURLRequest *copy = [request copy];
    @synchronized(NSURLRequest.class) {
        gDiscordTemplate = copy;
    }
}

static NSURLRequest *DiscordTemplate(void)
{
    @synchronized(NSURLRequest.class) {
        return gDiscordTemplate;
    }
}

static void CompleteDiscordRequest(uint64_t identifier, NSDictionary *result)
{
    NSData *json = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
    astral_ios_complete_network_request(identifier, AstralTextOfData(json ?: [NSData data]));
}

static NSDictionary *DiscordResult(NSData *data, NSURLResponse *response, NSError *error)
{
    NSHTTPURLResponse *http = [response isKindOfClass:NSHTTPURLResponse.class] ? (NSHTTPURLResponse *)response : nil;
    NSMutableDictionary *headers = [NSMutableDictionary dictionary];
    for (id name in http.allHeaderFields) {
        id value = http.allHeaderFields[name];
        if ([name isKindOfClass:NSString.class]) headers[name] = [value description];
    }
    NSString *body = data ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] : nil;
    NSMutableDictionary *result = [@{
        @"status" : @(http.statusCode),
        @"ok" : @((BOOL)(!error && http.statusCode >= 200 && http.statusCode < 300)),
        @"url" : http.URL.absoluteString ?: @"",
        @"headers" : headers,
        @"body" : body ?: @""
    } mutableCopy];
    if (error) result[@"error"] = @"request failed";
    return result;
}

@interface AstralNetworkFilter : NSURLProtocol
@property (nonatomic, strong) NSURLSessionDataTask *forwarded;
@end

@implementation AstralNetworkFilter

+ (BOOL)canInitWithRequest:(NSURLRequest *)request
{
    if ([NSURLProtocol propertyForKey:kHandledKey inRequest:request]) return NO;
    AstralRememberMediaRequest(request.URL);
    RememberDiscordTemplate(request);
    if (!request.URL) return NO;
    return AstralIsPickedVideoURL(request.URL) || WithProbe(request, YES).handles;
}

+ (BOOL)canInitWithTask:(NSURLSessionTask *)task
{
    if ([task isKindOfClass:NSClassFromString(@"NSURLSessionWebSocketTask")]) return NO;
    return task.currentRequest && [self canInitWithRequest:task.currentRequest];
}

+ (NSURLRequest *)canonicalRequestForRequest:(NSURLRequest *)request
{
    return request;
}

+ (NSURLSession *)forwardingSession
{
    static NSURLSession *session;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        NSURLSessionConfiguration *configuration = NSURLSessionConfiguration.defaultSessionConfiguration;
        configuration.protocolClasses = @[];
        session = [NSURLSession sessionWithConfiguration:configuration];
    });
    return session;
}

+ (void)sendDiscordRequest:(uint64_t)identifier method:(NSString *)method path:(NSString *)path body:(NSString *)body
{
    NSURLRequest *template = DiscordTemplate();
    NSString *base = template ? ApiBase(template.URL) : nil;
    NSURL *url = base ? [NSURL URLWithString:[base stringByAppendingString:path]] : nil;
    if (!url) {
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
            CompleteDiscordRequest(
                identifier,
                @{@"status" : @0,
                  @"ok" : @NO,
                  @"body" : @"",
                  @"error" : @"Discord has not called its API yet"});
        });
        return;
    }
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url
                                                           cachePolicy:template.cachePolicy
                                                       timeoutInterval:template.timeoutInterval];
    NSArray *dropped = DroppedTemplateHeaders();
    [template.allHTTPHeaderFields enumerateKeysAndObjectsUsingBlock:^(NSString *name, NSString *value, BOOL *stop) {
        if (![dropped containsObject:name.lowercaseString]) [request setValue:value forHTTPHeaderField:name];
    }];
    request.HTTPMethod = method;
    if (body.length) {
        request.HTTPBody = [body dataUsingEncoding:NSUTF8StringEncoding];
        [request setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
    }
    [[[AstralNetworkFilter forwardingSession] dataTaskWithRequest:request
                                                completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
                                                    CompleteDiscordRequest(identifier, DiscordResult(data, response, error));
                                                }] resume];
}

- (void)finishEmpty
{
    NSHTTPURLResponse *response = [[NSHTTPURLResponse alloc] initWithURL:self.request.URL
                                                              statusCode:kNoContent
                                                             HTTPVersion:@"HTTP/1.1"
                                                            headerFields:@{@"Content-Length" : @"0"}];
    [self.client URLProtocol:self didReceiveResponse:response cacheStoragePolicy:NSURLCacheStorageNotAllowed];
    [self.client URLProtocolDidFinishLoading:self];
}

- (void)deliverLocalResponse:(NSDictionary *)plain
{
    NSData *body = BodyOf(plain);
    [self.client URLProtocol:self
          didReceiveResponse:ResponseOf(self.request.URL, plain, body.length)
          cacheStoragePolicy:NSURLCacheStorageNotAllowed];
    [self.client URLProtocol:self didLoadData:body];
    [self.client URLProtocolDidFinishLoading:self];
}

- (void)forwardRequest
{
    NSMutableURLRequest *request = [self.request mutableCopy];
    MaterializeBody(request);
    id decision = AstralTakeJSON(astral_ios_network_begin(AstralTextOfData(JSONData(PlainRequest(request)))));
    NSDictionary *plan = [decision isKindOfClass:NSDictionary.class] ? decision : @{};
    if ([plan[@"block"] boolValue]) {
        [self finishEmpty];
        return;
    }
    if ([plan[@"respond"] isKindOfClass:NSDictionary.class]) {
        [self deliverLocalResponse:plan[@"respond"]];
        return;
    }
    if ([plan[@"request"] isKindOfClass:NSDictionary.class]) ApplyPlainRequest(request, plan[@"request"]);
    uint64_t exchange = [plan[@"exchange"] isKindOfClass:NSNumber.class] ? [plan[@"exchange"] unsignedLongLongValue] : 0;
    [NSURLProtocol setProperty:@YES forKey:kHandledKey inRequest:request];
    __weak AstralNetworkFilter *weakSelf = self;
    self.forwarded = [[AstralNetworkFilter forwardingSession]
        dataTaskWithRequest:request
          completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
              AstralNetworkFilter *strongSelf = weakSelf;
              NSHTTPURLResponse *http = [response isKindOfClass:NSHTTPURLResponse.class] ? (NSHTTPURLResponse *)response : nil;
              if (!strongSelf || error || !response) {
                  if (exchange) astral_ios_network_abandon(exchange);
                  [strongSelf.client URLProtocol:strongSelf
                                didFailWithError:error ?: [NSError errorWithDomain:NSURLErrorDomain code:NSURLErrorUnknown userInfo:nil]];
                  return;
              }
              NSData *body = data ?: [NSData data];
              NSURLResponse *delivered = response;
              if (exchange && http) {
                  id changed = AstralTakeJSON(astral_ios_network_finish(exchange, AstralTextOfData(JSONData(PlainResponse(http, body)))));
                  if ([changed isKindOfClass:NSDictionary.class]) {
                      body = BodyOf(changed);
                      delivered = ResponseOf(http.URL, changed, body.length);
                  }
              } else if (exchange) {
                  astral_ios_network_abandon(exchange);
              }
              [strongSelf.client URLProtocol:strongSelf didReceiveResponse:delivered cacheStoragePolicy:NSURLCacheStorageNotAllowed];
              [strongSelf.client URLProtocol:strongSelf didLoadData:body];
              [strongSelf.client URLProtocolDidFinishLoading:strongSelf];
          }];
    [self.forwarded resume];
}

- (void)deliverPickedFile:(NSURL *)file
{
    NSData *data = file ? [NSData dataWithContentsOfURL:file options:NSDataReadingMappedIfSafe error:nil] : nil;
    if (!data) {
        [self.client URLProtocol:self
                didFailWithError:[NSError errorWithDomain:NSURLErrorDomain code:NSURLErrorFileDoesNotExist userInfo:nil]];
        return;
    }
    NSRange range = AstralByteRange([self.request valueForHTTPHeaderField:@"Range"], data.length);
    BOOL partial = range.location != NSNotFound;
    if (!partial) range = NSMakeRange(0, data.length);
    NSMutableDictionary *headers = [@{
        @"Content-Type" : @"video/mp4",
        @"Accept-Ranges" : @"bytes",
        @"Content-Length" : [NSString stringWithFormat:@"%lu", (unsigned long)range.length],
    } mutableCopy];
    if (partial)
        headers[@"Content-Range"] = [NSString stringWithFormat:@"bytes %lu-%lu/%lu", (unsigned long)range.location,
                                                               (unsigned long)NSMaxRange(range) - 1, (unsigned long)data.length];
    NSHTTPURLResponse *response = [[NSHTTPURLResponse alloc] initWithURL:self.request.URL
                                                              statusCode:partial ? kPartialContent : kOk
                                                             HTTPVersion:@"HTTP/1.1"
                                                            headerFields:headers];
    [self.client URLProtocol:self didReceiveResponse:response cacheStoragePolicy:NSURLCacheStorageNotAllowed];
    [self.client URLProtocol:self didLoadData:[data subdataWithRange:range]];
    [self.client URLProtocolDidFinishLoading:self];
}

- (void)startLoading
{
    if (AstralIsPickedVideoURL(self.request.URL)) {
        NSThread *loading = NSThread.currentThread;
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            NSURL *file = AstralPickedVideoFile(self.request.URL);
            [self performSelector:@selector(deliverPickedFile:) onThread:loading withObject:file waitUntilDone:NO];
        });
        return;
    }
    if (PlanFor(self.request).block) {
        astral_ios_note_blocked_request();
        [self finishEmpty];
        return;
    }
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        [self forwardRequest];
    });
}

- (void)stopLoading
{
    [self.forwarded cancel];
    self.forwarded = nil;
}

@end

static NSURLSessionConfiguration *Filtered(NSURLSessionConfiguration *configuration)
{
    if (!configuration) return configuration;
    NSArray *classes = configuration.protocolClasses ?: @[];
    if ([classes containsObject:AstralNetworkFilter.class]) return configuration;
    NSURLSessionConfiguration *copy = [configuration copy];
    copy.protocolClasses = [@[ AstralNetworkFilter.class ] arrayByAddingObjectsFromArray:classes];
    return copy;
}

static id (*gOriginalSessionWithConfigurationDelegate)(id, SEL, NSURLSessionConfiguration *, id, NSOperationQueue *);

static id SessionWithConfigurationDelegate(id self, SEL selector, NSURLSessionConfiguration *configuration, id delegate,
                                           NSOperationQueue *queue)
{
    return gOriginalSessionWithConfigurationDelegate(self, selector, Filtered(configuration), delegate, queue);
}

static id (*gOriginalSessionWithConfiguration)(id, SEL, NSURLSessionConfiguration *);

static id SessionWithConfiguration(id self, SEL selector, NSURLSessionConfiguration *configuration)
{
    return gOriginalSessionWithConfiguration(self, selector, Filtered(configuration));
}

typedef void (^MessageHandler)(NSURLSessionWebSocketMessage *, NSError *);

static void (*gOriginalReceive)(id, SEL, MessageHandler);
static NSMutableArray<NSString *> *gInjected;
static NSString *const kInvalidSession = @"{\"op\":9,\"d\":false}";
static __weak id gGatewayTask;
static BOOL gGatewayUncompressed;
static BOOL gInvalidateAfterHello;

static NSURLSessionWebSocketMessage *WithStaffReady(NSURLSessionWebSocketMessage *message)
{
    if (!message || !StaffDevTools()) return message;
    if (message.type != NSURLSessionWebSocketMessageTypeString) {
        astral_ios_note_compressed_frame();
        return message;
    }
    NSString *marked = AstralTakeString(astral_ios_mark_staff_ready(AstralTextOf(message.string)));
    if (!marked) return message;
    return [[NSURLSessionWebSocketMessage alloc] initWithString:marked];
}

static NSString *NextInjected(void)
{
    @synchronized(NSMutableArray.class) {
        if (!gInjected.count) return nil;
        NSString *message = gInjected.firstObject;
        [gInjected removeObjectAtIndex:0];
        return message;
    }
}

static void QueueInjected(NSString *message)
{
    @synchronized(NSMutableArray.class) {
        if (!gInjected) gInjected = [NSMutableArray array];
        [gInjected addObject:message];
    }
}

static BOOL IsGatewayTask(id task)
{
    @synchronized(NSMutableArray.class) {
        return task && task == gGatewayTask;
    }
}

static void InvalidateAfterHello(id task)
{
    @synchronized(NSMutableArray.class) {
        if (!gInvalidateAfterHello || task != gGatewayTask || !gGatewayUncompressed) return;
        gInvalidateAfterHello = NO;
    }
    QueueInjected(kInvalidSession);
    AstralDevAppendLog(@"network filter: asked Discord for a new gateway session");
}

static void Receive(id self, SEL selector, MessageHandler handler)
{
    NSString *injected = handler && IsGatewayTask(self) ? NextInjected() : nil;
    if (injected) {
        injected = AstralTakeString(astral_ios_process_gateway(AstralTextOf(injected))) ?: injected;
        NSURLSessionWebSocketMessage *message = [[NSURLSessionWebSocketMessage alloc] initWithString:injected];
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            handler(message, nil);
        });
        return;
    }
    if (!handler) {
        gOriginalReceive(self, selector, handler);
        return;
    }
    gOriginalReceive(self, selector, ^(NSURLSessionWebSocketMessage *message, NSError *error) {
        NSURLSessionWebSocketMessage *forwarded = message;
        if (message.type == NSURLSessionWebSocketMessageTypeString) {
            NSString *rewritten = AstralTakeString(astral_ios_process_gateway(AstralTextOf(message.string)));
            if (rewritten) forwarded = [[NSURLSessionWebSocketMessage alloc] initWithString:rewritten];
        }
        handler(WithStaffReady(forwarded), error);
        if (!error) InvalidateAfterHello(self);
    });
}

static NSURL *WithoutGatewayCompression(NSURL *url)
{
    if (!url || !astral_ios_wants_uncompressed_gateway() || ![url.host hasPrefix:@"gateway"]) return url;
    NSURLComponents *components = [NSURLComponents componentsWithURL:url resolvingAgainstBaseURL:NO];
    NSMutableArray<NSURLQueryItem *> *items = [NSMutableArray array];
    for (NSURLQueryItem *item in components.queryItems) {
        if (![item.name isEqualToString:@"compress"]) [items addObject:item];
    }
    components.queryItems = items;
    AstralDevAppendLog([NSString stringWithFormat:@"network filter: gateway %@ -> %@", url.query ?: @"", components.query ?: @""]);
    return components.URL ?: url;
}

static id RememberGateway(id task, NSURL *requested, NSURL *opened)
{
    if (!task || ![requested.host hasPrefix:@"gateway"]) return task;
    @synchronized(NSMutableArray.class) {
        gGatewayTask = task;
        gGatewayUncompressed = ![opened.query containsString:@"compress="];
    }
    return task;
}

static id (*gOriginalWebSocketWithURL)(id, SEL, NSURL *);

static id WebSocketWithURL(id self, SEL selector, NSURL *url)
{
    NSURL *opened = WithoutGatewayCompression(url);
    return RememberGateway(gOriginalWebSocketWithURL(self, selector, opened), url, opened);
}

static id (*gOriginalWebSocketWithURLProtocols)(id, SEL, NSURL *, NSArray *);

static id WebSocketWithURLProtocols(id self, SEL selector, NSURL *url, NSArray *protocols)
{
    NSURL *opened = WithoutGatewayCompression(url);
    return RememberGateway(gOriginalWebSocketWithURLProtocols(self, selector, opened, protocols), url, opened);
}

static id (*gOriginalWebSocketWithRequest)(id, SEL, NSURLRequest *);

static id WebSocketWithRequest(id self, SEL selector, NSURLRequest *request)
{
    NSURL *url = WithoutGatewayCompression(request.URL);
    if (url == request.URL) return RememberGateway(gOriginalWebSocketWithRequest(self, selector, request), request.URL, url);
    NSMutableURLRequest *changed = [request mutableCopy];
    changed.URL = url;
    return RememberGateway(gOriginalWebSocketWithRequest(self, selector, changed), request.URL, url);
}

static void Swizzle(Class cls, SEL selector, IMP replacement, IMP *original)
{
    Method method = cls ? class_getInstanceMethod(cls, selector) : nil;
    if (!method) return;
    if (class_addMethod(cls, selector, replacement, method_getTypeEncoding(method))) {
        *original = method_getImplementation(method);
        return;
    }
    *original = method_setImplementation(method, replacement);
}

void AstralInstallNetworkFilter(BOOL (^staffDevTools)(void))
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gStaffDevTools = [staffDevTools copy];
        [NSURLProtocol registerClass:AstralNetworkFilter.class];
        Class sessionMeta = object_getClass(NSURLSession.class);
        Swizzle(sessionMeta, @selector(sessionWithConfiguration:delegate:delegateQueue:), (IMP)SessionWithConfigurationDelegate,
                (IMP *)&gOriginalSessionWithConfigurationDelegate);
        Swizzle(sessionMeta, @selector(sessionWithConfiguration:), (IMP)SessionWithConfiguration,
                (IMP *)&gOriginalSessionWithConfiguration);
        Class session = NSClassFromString(@"__NSURLSessionLocal") ?: NSURLSession.class;
        Swizzle(session, @selector(webSocketTaskWithURL:), (IMP)WebSocketWithURL, (IMP *)&gOriginalWebSocketWithURL);
        Swizzle(session, @selector(webSocketTaskWithURL:protocols:), (IMP)WebSocketWithURLProtocols,
                (IMP *)&gOriginalWebSocketWithURLProtocols);
        Swizzle(session, @selector(webSocketTaskWithRequest:), (IMP)WebSocketWithRequest, (IMP *)&gOriginalWebSocketWithRequest);
        Class webSocket = NSClassFromString(@"__NSURLSessionWebSocketTask") ?: NSClassFromString(@"NSURLSessionWebSocketTask");
        Swizzle(webSocket, @selector(receiveMessageWithCompletionHandler:), (IMP)Receive, (IMP *)&gOriginalReceive);
    });
}

BOOL AstralDispatchLocalGatewayEvent(NSString *message)
{
    BOOL uncompressed;
    @synchronized(NSMutableArray.class) {
        uncompressed = gGatewayTask != nil && gGatewayUncompressed;
    }
    if (!uncompressed || !message.length) return NO;
    QueueInjected(message);
    return YES;
}

BOOL AstralInjectGatewayMessage(NSString *message)
{
    if (!StaffDevTools() || !message.length) return NO;
    QueueInjected(message);
    return YES;
}

void AstralRestartGatewaySession(void)
{
    id task;
    BOOL uncompressed;
    @synchronized(NSMutableArray.class) {
        task = gGatewayTask;
        uncompressed = gGatewayUncompressed;
    }
    if (!task) return;
    if (uncompressed && !StaffDevTools()) {
        QueueInjected(kInvalidSession);
        AstralDevAppendLog(@"network filter: asked Discord for a new gateway session");
        return;
    }
    @synchronized(NSMutableArray.class) {
        gInvalidateAfterHello = YES;
    }
    [task cancelWithCloseCode:NSURLSessionWebSocketCloseCodeGoingAway reason:nil];
    AstralDevAppendLog(@"network filter: closed the gateway so it reconnects with the new settings");
}

void AstralSendDiscordRequest(uint64_t identifier, NSString *method, NSString *path, NSString *body)
{
    [AstralNetworkFilter sendDiscordRequest:identifier method:method path:path body:body];
}
