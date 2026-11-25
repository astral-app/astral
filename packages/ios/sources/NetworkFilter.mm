#import "NetworkFilter.h"

#import <objc/runtime.h>
#import <atomic>
#import <string>

#import "AstralLuaHost.h"
#import "AstralNetworkPolicy.h"
#import "DevServer.h"

static NSString *const kHandledKey = @"app.astral.network-filter";

static BOOL (^gStaffDevTools)(void);
static std::atomic<unsigned long long> gBlocked;
static std::atomic<unsigned long long> gStaffUsers;
static std::atomic<unsigned long long> gStaffReadies;
static std::atomic<unsigned long long> gCompressedFrames;
static std::atomic<unsigned long long> gDiscordRequests;
static std::atomic<unsigned long long> gMessageRequests;

static BOOL StaffDevTools(void)
{
    return gStaffDevTools && gStaffDevTools();
}

static std::string Text(NSString *value)
{
    return value ? std::string(value.UTF8String ?: "") : std::string();
}

static BOOL IsTelemetry(NSURL *url)
{
    return url && astral::isTelemetryRequest(Text(url.host), Text(url.path));
}

static BOOL IsCurrentUser(NSURLRequest *request)
{
    NSURL *url = request.URL;
    BOOL reading = !request.HTTPMethod || [request.HTTPMethod isEqualToString:@"GET"];
    return reading && url && astral::isCurrentUserRequest(Text(url.host), Text(url.path));
}

static const NSUInteger kMaxMessageBody = 1u << 20;

static BOOL IsJSONMessageRequest(NSURLRequest *request)
{
    if (!request.HTTPBody && !request.HTTPBodyStream) return NO;
    if (request.HTTPBody.length > kMaxMessageBody) return NO;
    NSString *contentType = [[request valueForHTTPHeaderField:@"Content-Type"] lowercaseString];
    return [contentType containsString:@"application/json"];
}

static BOOL IsOutgoingMessage(NSURLRequest *request)
{
    return request.URL && IsJSONMessageRequest(request) && astral::luahost::hasOutgoingMessageTransforms() &&
           astral::outgoingMessageTarget(Text(request.URL.host), Text(request.URL.path), Text(request.HTTPMethod)).has_value();
}

static NSData *StreamedBody(NSInputStream *stream)
{
    NSMutableData *body = [NSMutableData data];
    uint8_t buffer[16384];
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

static void TransformOutgoingMessage(NSMutableURLRequest *request)
{
    MaterializeBody(request);
    if (!request.URL || !IsJSONMessageRequest(request)) return;
    auto transformed = astral::transformOutgoingMessageBody(Text(request.URL.host), Text(request.URL.path), Text(request.HTTPMethod),
                                                            std::string_view((const char *)request.HTTPBody.bytes, request.HTTPBody.length),
                                                            [](const astral::OutgoingMessageTarget &target, std::string_view content) {
                                                                return astral::luahost::transformOutgoingMessage(
                                                                    target.channel, target.operation, std::string(content));
                                                            });
    if (!transformed) return;
    request.HTTPBody = [NSData dataWithBytes:transformed->data() length:transformed->size()];
    [request setValue:nil forHTTPHeaderField:@"Content-Length"];
    [request setValue:nil forHTTPHeaderField:@"Transfer-Encoding"];
}

static BOOL WantsHttp(NSURLRequest *request)
{
    NSURL *url = request.URL;
    return url && astral::luahost::wantsHttp(Text(request.HTTPMethod ?: @"GET"), Text(url.host.lowercaseString),
                                             Text(url.path.length ? url.path : @"/"));
}

static astral::luahost::HttpRequest HttpRequestOf(NSURLRequest *request)
{
    astral::luahost::HttpRequest plain;
    plain.method = Text(request.HTTPMethod ?: @"GET");
    plain.url = Text(request.URL.absoluteString);
    plain.path = Text(request.URL.path.length ? request.URL.path : @"/");
    NSDictionary<NSString *, NSString *> *fields = request.allHTTPHeaderFields;
    for (NSString *name in fields) plain.headers.emplace_back(Text(name.lowercaseString), Text(fields[name]));
    if (request.HTTPBody) plain.body.assign((const char *)request.HTTPBody.bytes, request.HTTPBody.length);
    return plain;
}

static void ApplyHttpRequest(NSMutableURLRequest *request, const astral::luahost::HttpRequest &plain)
{
    NSURL *url = [NSURL URLWithString:[NSString stringWithUTF8String:plain.url.c_str()] ?: @""];
    if (url) request.URL = url;
    request.HTTPMethod = [NSString stringWithUTF8String:plain.method.c_str()] ?: request.HTTPMethod;
    NSMutableDictionary<NSString *, NSString *> *headers = [NSMutableDictionary dictionary];
    for (const auto &[name, value] : plain.headers) {
        NSString *key = [NSString stringWithUTF8String:name.c_str()];
        NSString *text = [NSString stringWithUTF8String:value.c_str()];
        if (key && text) headers[key] = text;
    }
    [headers removeObjectForKey:@"content-length"];
    request.allHTTPHeaderFields = headers;
    request.HTTPBodyStream = nil;
    request.HTTPBody = [NSData dataWithBytes:plain.body.data() length:plain.body.size()];
}

static NSHTTPURLResponse *HttpResponseOf(NSURL *url, const astral::luahost::HttpResponse &plain)
{
    NSMutableDictionary<NSString *, NSString *> *headers = [NSMutableDictionary dictionary];
    for (const auto &[name, value] : plain.headers) {
        NSString *key = [NSString stringWithUTF8String:name.c_str()];
        NSString *text = [NSString stringWithUTF8String:value.c_str()];
        if (key && text && ![key isEqualToString:@"content-encoding"]) headers[key] = text;
    }
    headers[@"Content-Length"] = [NSString stringWithFormat:@"%zu", plain.body.size()];
    return [[NSHTTPURLResponse alloc] initWithURL:url statusCode:plain.status HTTPVersion:@"HTTP/1.1" headerFields:headers];
}

@interface AstralNetworkFilter : NSURLProtocol
@property (nonatomic, strong) NSURLSessionDataTask *forwarded;
@end

@implementation AstralNetworkFilter

+ (BOOL)canInitWithRequest:(NSURLRequest *)request
{
    if ([NSURLProtocol propertyForKey:kHandledKey inRequest:request]) return NO;
    if (request.URL && astral::isDiscordHost(Text(request.URL.host))) {
        gDiscordRequests++;
        if ([request.HTTPMethod isEqualToString:@"POST"] && [request.URL.path hasSuffix:@"/messages"]) gMessageRequests++;
    }
    return IsTelemetry(request.URL) || (StaffDevTools() && IsCurrentUser(request)) || IsOutgoingMessage(request) || WantsHttp(request);
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

- (void)finishEmpty
{
    NSHTTPURLResponse *response = [[NSHTTPURLResponse alloc] initWithURL:self.request.URL
                                                              statusCode:204
                                                             HTTPVersion:@"HTTP/1.1"
                                                            headerFields:@{@"Content-Length" : @"0"}];
    [self.client URLProtocol:self didReceiveResponse:response cacheStoragePolicy:NSURLCacheStorageNotAllowed];
    [self.client URLProtocolDidFinishLoading:self];
}

- (void)deliverLocalResponse:(const astral::luahost::HttpResponse &)plain
{
    NSData *body = [NSData dataWithBytes:plain.body.data() length:plain.body.size()];
    [self.client URLProtocol:self
          didReceiveResponse:HttpResponseOf(self.request.URL, plain)
          cacheStoragePolicy:NSURLCacheStorageNotAllowed];
    [self.client URLProtocol:self didLoadData:body];
    [self.client URLProtocolDidFinishLoading:self];
}

- (void)forwardRequest
{
    NSMutableURLRequest *request = [self.request mutableCopy];
    TransformOutgoingMessage(request);
    BOOL intercepted = WantsHttp(request);
    astral::luahost::HttpRequest plain;
    if (intercepted) {
        MaterializeBody(request);
        plain = HttpRequestOf(request);
        auto decision = astral::luahost::interceptHttpRequest(plain);
        if (decision.response) {
            [self deliverLocalResponse:*decision.response];
            return;
        }
        if (decision.changed) ApplyHttpRequest(request, plain);
    }
    [NSURLProtocol setProperty:@YES forKey:kHandledKey inRequest:request];
    BOOL markStaffResponse = StaffDevTools() && IsCurrentUser(self.request);
    __weak AstralNetworkFilter *weakSelf = self;
    self.forwarded = [[AstralNetworkFilter forwardingSession]
        dataTaskWithRequest:request
          completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
              AstralNetworkFilter *strongSelf = weakSelf;
              if (!strongSelf) return;
              if (error || !response) {
                  [strongSelf.client URLProtocol:strongSelf
                                didFailWithError:error ?: [NSError errorWithDomain:NSURLErrorDomain code:NSURLErrorUnknown userInfo:nil]];
                  return;
              }
              NSData *body = data ?: [NSData data];
              NSURLResponse *delivered = response;
              NSHTTPURLResponse *http = [response isKindOfClass:NSHTTPURLResponse.class] ? (NSHTTPURLResponse *)response : nil;
              if (markStaffResponse && http.statusCode == 200 && body.length) {
                  auto marked = astral::withStaffUser(std::string_view((const char *)body.bytes, body.length));
                  if (marked) {
                      body = [NSData dataWithBytes:marked->data() length:marked->size()];
                      NSMutableDictionary *headers = [http.allHeaderFields mutableCopy];
                      [headers removeObjectForKey:@"Content-Encoding"];
                      headers[@"Content-Length"] = [NSString stringWithFormat:@"%lu", (unsigned long)body.length];
                      delivered = [[NSHTTPURLResponse alloc] initWithURL:http.URL
                                                              statusCode:200
                                                             HTTPVersion:@"HTTP/1.1"
                                                            headerFields:headers];
                      gStaffUsers++;
                  }
              }
              if (intercepted && http) {
                  astral::luahost::HttpResponse received;
                  received.status = (int)http.statusCode;
                  NSDictionary *fields = http.allHeaderFields;
                  for (NSString *name in fields)
                      received.headers.emplace_back(Text(name.lowercaseString), Text([fields[name] description]));
                  received.body.assign((const char *)body.bytes, body.length);
                  if (astral::luahost::interceptHttpResponse(plain, received)) {
                      body = [NSData dataWithBytes:received.body.data() length:received.body.size()];
                      delivered = HttpResponseOf(http.URL, received);
                  }
              }
              [strongSelf.client URLProtocol:strongSelf didReceiveResponse:delivered cacheStoragePolicy:NSURLCacheStorageNotAllowed];
              [strongSelf.client URLProtocol:strongSelf didLoadData:body];
              [strongSelf.client URLProtocolDidFinishLoading:strongSelf];
          }];
    [self.forwarded resume];
}

- (void)startLoading
{
    if (IsTelemetry(self.request.URL)) {
        gBlocked++;
        [self finishEmpty];
        return;
    }
    if (!WantsHttp(self.request)) {
        [self forwardRequest];
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
        gCompressedFrames++;
        return message;
    }
    auto marked = astral::withStaffReady(Text(message.string));
    if (!marked) return message;
    gStaffReadies++;
    AstralDevAppendLog(@"network filter: marked the account as staff in READY");
    return [[NSURLSessionWebSocketMessage alloc] initWithString:[NSString stringWithUTF8String:marked->c_str()] ?: message.string];
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
            std::string contents = Text(message.string);
            auto processed = astral::luahost::processGatewayMessage(contents);
            if (processed) {
                NSString *rewritten = [NSString stringWithUTF8String:processed->c_str()];
                if (rewritten) forwarded = [[NSURLSessionWebSocketMessage alloc] initWithString:rewritten];
            }
        }
        handler(WithStaffReady(forwarded), error);
        if (!error) InvalidateAfterHello(self);
    });
}

static NSURL *WithoutGatewayCompression(NSURL *url)
{
    if (!url || !(StaffDevTools() || astral::luahost::needsGatewayEvents()) || ![url.host hasPrefix:@"gateway"]) return url;
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
        gGatewayUncompressed = ![opened isEqual:requested];
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

NSDictionary *AstralNetworkFilterStats(void)
{
    return @{
        @"blocked" : @(gBlocked.load()),
        @"staffUsers" : @(gStaffUsers.load()),
        @"staffReadies" : @(gStaffReadies.load()),
        @"compressedFrames" : @(gCompressedFrames.load()),
        @"discordRequests" : @(gDiscordRequests.load()),
        @"messageRequests" : @(gMessageRequests.load()),
    };
}
