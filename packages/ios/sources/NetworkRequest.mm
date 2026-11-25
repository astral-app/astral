#import "NetworkRequest.h"

#import "AstralLuaHost.h"
#import "FoundationJSON.h"

#include <algorithm>
#include <cctype>

static constexpr NSUInteger kMaximumResponseBytes = 1u << 20;
static NSLock *gRequestsLock;
static NSMutableDictionary<NSNumber *, id> *gRequests;

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
        @"ok" : @(self.response.statusCode >= 200 && self.response.statusCode < 300 && !self.bodyTooLarge && !error),
        @"url" : self.response.URL.absoluteString ?: @"",
        @"headers" : headers,
        @"body" : self.bodyTooLarge ? @"" : body
    } mutableCopy];
    if (!text && !self.bodyTooLarge && self.body.length) result[@"body_base64"] = [self.body base64EncodedStringWithOptions:0];
    if (self.bodyTooLarge)
        result[@"error"] = @"response body exceeds 1 MiB";
    else if (error)
        result[@"error"] = @"request failed";
    astral::luahost::completeNetworkRequest(self.identifier, AstralJSONFromObject(result));
    [self.session finishTasksAndInvalidate];
}

@end

void astral::startNetworkRequest(uint64_t requestID, const nlohmann::json &options)
{
    PrepareRequestStorage();
    NSString *url = [NSString stringWithUTF8String:options.value("url", "").c_str()];
    NSURL *target = [NSURL URLWithString:url];
    if (!target || target.host.length == 0 || ![target.scheme.lowercaseString isEqualToString:@"https"]) {
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
            luahost::completeNetworkRequest(requestID, {{"status", 0}, {"ok", false}, {"body", ""}, {"error", "invalid https URL"}});
        });
        return;
    }
    NSMutableURLRequest *nativeRequest = [NSMutableURLRequest requestWithURL:target
                                                                 cachePolicy:NSURLRequestReloadIgnoringLocalCacheData
                                                             timeoutInterval:options.value("timeout_ms", 15000) / 1000.0];
    nativeRequest.HTTPMethod = [NSString stringWithUTF8String:options.value("method", "GET").c_str()];
    nativeRequest.HTTPShouldHandleCookies = NO;
    nlohmann::json headers = options.value("headers", nlohmann::json::object());
    bool contentTypeProvided = false;
    for (auto entry = headers.begin(); entry != headers.end(); ++entry) {
        if (entry.value().is_string()) {
            nativeRequest.allHTTPHeaderFields = nativeRequest.allHTTPHeaderFields ?: @{};
            [nativeRequest setValue:[NSString stringWithUTF8String:entry.value().get<std::string>().c_str()]
                 forHTTPHeaderField:[NSString stringWithUTF8String:entry.key().c_str()]];
            std::string name = entry.key();
            std::transform(name.begin(), name.end(), name.begin(), [](unsigned char character) { return (char)std::tolower(character); });
            if (name == "content-type") contentTypeProvided = true;
        }
    }
    if (options.contains("body") && options["body"].is_string()) {
        const std::string &body = options["body"].get_ref<const std::string &>();
        nativeRequest.HTTPBody = [NSData dataWithBytes:body.data() length:body.size()];
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

void astral::cancelNetworkRequest(uint64_t requestID)
{
    if (!gRequestsLock) return;
    [gRequestsLock lock];
    AstralNetworkRequest *request = gRequests[@(requestID)];
    [gRequestsLock unlock];
    [request.task cancel];
}
