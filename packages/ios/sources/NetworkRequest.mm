#import "NetworkRequest.h"

#import "AstralCore.h"

static const NSUInteger kMaximumResponseBytes = 1u << 20;
static const double kDefaultTimeoutMs = 15000;
static NSLock *gRequestsLock;
static NSMutableDictionary<NSNumber *, id> *gRequests;

static void CompleteRequest(uint64_t identifier, NSDictionary *result)
{
    NSData *json = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
    astral_ios_complete_network_request(identifier, AstralTextOfData(json ?: [NSData data]));
}

static void PrepareRequestStorage(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        gRequestsLock = [NSLock new];
        gRequests = [NSMutableDictionary dictionary];
    });
}

@interface AstralNetworkRequest : NSObject <NSURLSessionDataDelegate, NSURLSessionTaskDelegate>
@property (nonatomic) uint64_t identifier;
@property (nonatomic, strong) NSURLSession *session;
@property (nonatomic, strong) NSURLSessionDataTask *task;
@property (nonatomic, strong) NSMutableData *body;
@property (nonatomic, strong) NSHTTPURLResponse *response;
@property (nonatomic) BOOL bodyTooLarge;
@end

@implementation AstralNetworkRequest

- (void)URLSession:(NSURLSession *)session
              dataTask:(NSURLSessionDataTask *)dataTask
    didReceiveResponse:(NSURLResponse *)response
     completionHandler:(void (^)(NSURLSessionResponseDisposition))completionHandler
{
    self.response = [response isKindOfClass:NSHTTPURLResponse.class] ? (NSHTTPURLResponse *)response : nil;
    completionHandler(NSURLSessionResponseAllow);
}

- (void)URLSession:(NSURLSession *)session dataTask:(NSURLSessionDataTask *)dataTask didReceiveData:(NSData *)data
{
    if (self.body.length + data.length > kMaximumResponseBytes) {
        self.bodyTooLarge = YES;
        [dataTask cancel];
        return;
    }
    [self.body appendData:data];
}

- (void)URLSession:(NSURLSession *)session
                          task:(NSURLSessionTask *)task
    willPerformHTTPRedirection:(NSHTTPURLResponse *)response
                    newRequest:(NSURLRequest *)request
             completionHandler:(void (^)(NSURLRequest *))completionHandler
{
    completionHandler(nil);
}

- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task didCompleteWithError:(NSError *)error
{
    NSNumber *key = @(self.identifier);
    [gRequestsLock lock];
    [gRequests removeObjectForKey:key];
    [gRequestsLock unlock];
    NSMutableDictionary *headers = [NSMutableDictionary dictionary];
    for (id name in self.response.allHeaderFields) {
        id value = self.response.allHeaderFields[name];
        if ([name isKindOfClass:NSString.class] && [value respondsToSelector:@selector(description)]) headers[name] = [value description];
    }
    NSString *text = [[NSString alloc] initWithData:self.body encoding:NSUTF8StringEncoding];
    NSString *body = text ?: @"";
    NSMutableDictionary *result = [@{
        @"status" : @(self.response.statusCode),
        @"ok" : @((BOOL)(self.response.statusCode >= 200 && self.response.statusCode < 300 && !self.bodyTooLarge && !error)),
        @"url" : self.response.URL.absoluteString ?: @"",
        @"headers" : headers,
        @"body" : self.bodyTooLarge ? @"" : body
    } mutableCopy];
    if (!text && !self.bodyTooLarge && self.body.length) result[@"body_base64"] = [self.body base64EncodedStringWithOptions:0];
    if (self.bodyTooLarge)
        result[@"error"] = @"response body exceeds 1 MiB";
    else if (error)
        result[@"error"] = @"request failed";
    CompleteRequest(self.identifier, result);
    [self.session finishTasksAndInvalidate];
}

@end

static NSString *TextOption(NSDictionary *options, NSString *key, NSString *fallback)
{
    id value = options[key];
    return [value isKindOfClass:NSString.class] ? value : fallback;
}

void AstralStartNetworkRequest(uint64_t requestID, NSDictionary *options)
{
    PrepareRequestStorage();
    NSURL *target = [NSURL URLWithString:TextOption(options, @"url", @"")];
    if (!target || target.host.length == 0 || ![target.scheme.lowercaseString isEqualToString:@"https"]) {
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
            CompleteRequest(requestID, @{@"status" : @0, @"ok" : @NO, @"body" : @"", @"error" : @"invalid https URL"});
        });
        return;
    }
    id timeout = options[@"timeout_ms"];
    double timeoutMs = [timeout isKindOfClass:NSNumber.class] ? [timeout doubleValue] : kDefaultTimeoutMs;
    NSMutableURLRequest *nativeRequest = [NSMutableURLRequest requestWithURL:target
                                                                 cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                                             timeoutInterval:timeoutMs / 1000.0];
    nativeRequest.HTTPMethod = TextOption(options, @"method", @"GET");
    nativeRequest.HTTPShouldHandleCookies = NO;
    NSDictionary *headers = [options[@"headers"] isKindOfClass:NSDictionary.class] ? options[@"headers"] : @{};
    BOOL contentTypeProvided = NO;
    for (NSString *name in headers) {
        id value = headers[name];
        if (![name isKindOfClass:NSString.class] || ![value isKindOfClass:NSString.class]) continue;
        [nativeRequest setValue:value forHTTPHeaderField:name];
        if ([name.lowercaseString isEqualToString:@"content-type"]) contentTypeProvided = YES;
    }
    id body = options[@"body"];
    if ([body isKindOfClass:NSString.class]) {
        nativeRequest.HTTPBody = [body dataUsingEncoding:NSUTF8StringEncoding];
        if (!contentTypeProvided) [nativeRequest setValue:@"application/octet-stream" forHTTPHeaderField:@"Content-Type"];
    }
    NSURLSessionConfiguration *configuration = [NSURLSessionConfiguration ephemeralSessionConfiguration];
    configuration.HTTPCookieStorage = nil;
    configuration.HTTPShouldSetCookies = NO;
    configuration.URLCache = nil;
    AstralNetworkRequest *delegate = [AstralNetworkRequest new];
    delegate.identifier = requestID;
    delegate.body = [NSMutableData data];
    delegate.session = [NSURLSession sessionWithConfiguration:configuration delegate:delegate delegateQueue:nil];
    delegate.task = [delegate.session dataTaskWithRequest:nativeRequest];
    [gRequestsLock lock];
    gRequests[@(requestID)] = delegate;
    [gRequestsLock unlock];
    [delegate.task resume];
}

void AstralCancelNetworkRequest(uint64_t requestID)
{
    if (!gRequestsLock) return;
    [gRequestsLock lock];
    AstralNetworkRequest *request = gRequests[@(requestID)];
    [gRequestsLock unlock];
    [request.task cancel];
}
