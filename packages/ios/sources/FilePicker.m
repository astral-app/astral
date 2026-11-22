#import "FilePicker.h"

#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

#import "PhoenixImage.h"

@interface AstralFilePicker : NSObject <UIDocumentPickerDelegate>
@property (nonatomic, copy) void (^completion)(NSData *, NSString *);
@end

static AstralFilePicker *gPicker;

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
    BOOL scoped = [url startAccessingSecurityScopedResource];
    NSData *data = [NSData dataWithContentsOfURL:url];
    if (scoped) [url stopAccessingSecurityScopedResource];
    self.completion(data, url.lastPathComponent.stringByDeletingPathExtension);
    gPicker = nil;
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller
{
    gPicker = nil;
}
@end

void AstralPickFile(NSArray<NSString *> *extensions, void (^completion)(NSData *, NSString *))
{
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
    [TopController() presentViewController:picker animated:YES completion:nil];
}

void AstralShowAlert(NSString *title, NSString *message)
{
    UIAlertController *alert = [UIAlertController alertControllerWithTitle:title
                                                                   message:message
                                                            preferredStyle:UIAlertControllerStyleAlert];
    [alert addAction:[UIAlertAction actionWithTitle:@"OK" style:UIAlertActionStyleDefault handler:nil]];
    [TopController() presentViewController:alert animated:YES completion:nil];
}
