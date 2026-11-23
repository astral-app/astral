#import "FilePicker.h"

#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <objc/runtime.h>

#import "PhoenixImage.h"

@interface AstralFilePicker : NSObject <UIDocumentPickerDelegate>
@property (nonatomic, copy) void (^completion)(NSData *, NSString *);
@property (nonatomic, copy) void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *);
@property (nonatomic) uint64_t request;
@property (nonatomic, weak) UIViewController *controller;
@end

static AstralFilePicker *gPicker;
static IMP gOriginalStartAccessing;

static BOOL StartAccessingResource(id object, SEL selector)
{
    ((BOOL (*)(id, SEL))gOriginalStartAccessing)(object, selector);
    return YES;
}

static void InstallSecurityScopedResourceCompatibility(void)
{
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        Method method = class_getInstanceMethod(NSURL.class, @selector(startAccessingSecurityScopedResource));
        if (method) gOriginalStartAccessing = method_setImplementation(method, (IMP)StartAccessingResource);
    });
}

static UIViewController *TopController(void)
{
    UIViewController *controller = AstralPhoenixWindow().rootViewController;
    while (controller.presentedViewController) controller = controller.presentedViewController;
    return controller;
}

@implementation AstralFilePicker
- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls
{
    NSURL *url = urls.firstObject;
    NSString *error = nil;
    NSData *data = nil;
    if (!url) {
        error = @"file_unavailable";
    }
    BOOL scoped = [url startAccessingSecurityScopedResource];
    NSFileHandle *handle = url ? [NSFileHandle fileHandleForReadingFromURL:url error:nil] : nil;
    data = [handle readDataOfLength:(1u << 20) + 1];
    if (data.length > (1u << 20)) {
        data = nil;
        error = @"file_too_large";
    } else if (!data && !error) {
        error = @"file_unreadable";
    }
    [handle closeFile];
    if (scoped) [url stopAccessingSecurityScopedResource];
    NSString *name = url.lastPathComponent;
    void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *) = self.pluginCompletion;
    void (^completion)(NSData *, NSString *) = self.completion;
    uint64_t request = self.request;
    gPicker = nil;
    if (pluginCompletion) pluginCompletion(request, data, name, error);
    if (completion) completion(data, name);
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller
{
    void (^pluginCompletion)(uint64_t, NSData *, NSString *, NSString *) = self.pluginCompletion;
    uint64_t request = self.request;
    gPicker = nil;
    if (pluginCompletion) pluginCompletion(request, nil, nil, @"cancelled");
}
@end

void AstralPickFile(NSArray<NSString *> *extensions, void (^completion)(NSData *, NSString *))
{
    InstallSecurityScopedResourceCompatibility();
    if (gPicker) {
        completion(nil, nil);
        return;
    }
    NSMutableArray<UTType *> *types = [NSMutableArray array];
    for (NSString *extension in extensions) {
        UTType *type = [UTType typeWithFilenameExtension:extension];
        if (type) [types addObject:type];
    }
    [types addObjectsFromArray:@[ UTTypePlainText, UTTypeData ]];
    gPicker = [AstralFilePicker new];
    gPicker.completion = completion;
    UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:types];
    picker.delegate = gPicker;
    UIViewController *top = TopController();
    if (!top) {
        gPicker = nil;
        completion(nil, nil);
        return;
    }
    [top presentViewController:picker animated:YES completion:nil];
}

BOOL AstralPickPluginFile(uint64_t request, NSArray<NSString *> *extensions, void (^completion)(uint64_t, NSData *, NSString *, NSString *))
{
    InstallSecurityScopedResourceCompatibility();
    dispatch_async(dispatch_get_main_queue(), ^{
        if (gPicker) {
            completion(request, nil, nil, @"picker_busy");
            return;
        }
        UIViewController *top = TopController();
        if (!top) {
            completion(request, nil, nil, @"picker_unavailable");
            return;
        }
        NSMutableArray<UTType *> *types = [NSMutableArray array];
        for (NSString *extension in extensions) {
            UTType *type = [UTType typeWithFilenameExtension:extension];
            if (type) [types addObject:type];
        }
        if (types.count == 0) [types addObjectsFromArray:@[ UTTypePlainText, UTTypeData ]];
        AstralFilePicker *pickerDelegate = [AstralFilePicker new];
        pickerDelegate.request = request;
        pickerDelegate.pluginCompletion = completion;
        gPicker = pickerDelegate;
        UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:types];
        picker.delegate = pickerDelegate;
        pickerDelegate.controller = picker;
        [top presentViewController:picker animated:YES completion:nil];
    });
    return YES;
}

void AstralCancelPluginFile(uint64_t request)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!gPicker || gPicker.request != request) return;
        AstralFilePicker *picker = gPicker;
        gPicker = nil;
        picker.pluginCompletion = nil;
        [picker.controller dismissViewControllerAnimated:NO completion:nil];
    });
}

void AstralShowAlert(NSString *title, NSString *message)
{
    dispatch_async(dispatch_get_main_queue(), ^{
        UIViewController *top = TopController();
        if (!top) return;
        UIAlertController *alert = [UIAlertController alertControllerWithTitle:title
                                                                       message:message
                                                                preferredStyle:UIAlertControllerStyleAlert];
        [alert addAction:[UIAlertAction actionWithTitle:@"OK" style:UIAlertActionStyleDefault handler:nil]];
        [top presentViewController:alert animated:YES completion:nil];
    });
}
