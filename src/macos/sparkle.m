#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <objc/runtime.h>

static id gController;

static BOOL folio_sparkle_load(void) {
    NSString *path = [[[NSBundle mainBundle] privateFrameworksPath]
        stringByAppendingPathComponent:@"Sparkle.framework"];
    NSBundle *sparkle = [NSBundle bundleWithPath:path];
    return sparkle != nil && [sparkle load];
}

void folio_sparkle_start(void) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        if (!folio_sparkle_load()) {
            return;
        }
        Class cls = NSClassFromString(@"SPUStandardUpdaterController");
        if (cls == Nil) {
            return;
        }
        id allocated = ((id(*)(Class, SEL))objc_msgSend)(cls, sel_registerName("alloc"));
        gController = ((id(*)(id, SEL, BOOL, id, id))objc_msgSend)(
            allocated,
            sel_registerName("initWithStartingUpdater:updaterDelegate:userDriverDelegate:"),
            YES,
            nil,
            nil);
    });
}

int folio_sparkle_available(void) {
    folio_sparkle_start();
    return gController != nil ? 1 : 0;
}

void folio_sparkle_check(void) {
    folio_sparkle_start();
    if (gController == nil) {
        return;
    }
    ((void (*)(id, SEL, id))objc_msgSend)(
        gController, sel_registerName("checkForUpdates:"), nil);
}
