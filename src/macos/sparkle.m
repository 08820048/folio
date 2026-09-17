#import <Foundation/Foundation.h>
#import <objc/message.h>
#import <objc/runtime.h>

static id gController;
static id gDelegate;

// Result of the last background information check. Read from Rust through
// folio_sparkle_check_state / folio_sparkle_found_*; the strings stay valid
// until the next check replaces them.
static int gCheckState; // 0 running/unknown, 1 up to date (or failed), 2 update found
static NSString *gFoundVersion;
static NSString *gFoundBuild;

// SPUUpdaterDelegate. Sparkle's headers are not linked into this build, so
// the delegate is a plain NSObject with the selectors Sparkle looks for.
@interface FolioUpdaterDelegate : NSObject
@end

@implementation FolioUpdaterDelegate

- (void)updater:(id)updater didFindValidUpdate:(id)item {
    id version = ((id(*)(id, SEL))objc_msgSend)(item, sel_registerName("displayVersionString"));
    id build = ((id(*)(id, SEL))objc_msgSend)(item, sel_registerName("versionString"));
    gFoundVersion = [version isKindOfClass:[NSString class]] ? [version copy] : @"";
    gFoundBuild = [build isKindOfClass:[NSString class]] ? [build copy] : @"";
    gCheckState = 2;
}

- (void)updaterDidNotFindUpdate:(id)updater error:(NSError *)error {
    gCheckState = 1;
}

- (void)updaterDidNotFindUpdate:(id)updater {
    gCheckState = 1;
}

- (void)updater:(id)updater didAbortWithError:(NSError *)error {
    if (gCheckState == 0) {
        gCheckState = 1;
    }
}

- (void)updater:(id)updater didFinishUpdateCycleForUpdateCheck:(NSInteger)check error:(NSError *)error {
    if (gCheckState == 0) {
        gCheckState = 1;
    }
}

@end

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
        gDelegate = [FolioUpdaterDelegate new];
        id allocated = ((id(*)(Class, SEL))objc_msgSend)(cls, sel_registerName("alloc"));
        gController = ((id(*)(id, SEL, BOOL, id, id))objc_msgSend)(
            allocated,
            sel_registerName("initWithStartingUpdater:updaterDelegate:userDriverDelegate:"),
            YES,
            gDelegate,
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

// Ask the feed without showing any UI. The answer lands in the delegate;
// poll folio_sparkle_check_state for it.
void folio_sparkle_check_information(void) {
    folio_sparkle_start();
    if (gController == nil) {
        gCheckState = 1;
        return;
    }
    id updater = ((id(*)(id, SEL))objc_msgSend)(gController, sel_registerName("updater"));
    if (updater == nil) {
        gCheckState = 1;
        return;
    }
    gCheckState = 0;
    gFoundVersion = nil;
    gFoundBuild = nil;
    ((void (*)(id, SEL))objc_msgSend)(updater, sel_registerName("checkForUpdateInformation"));
}

int folio_sparkle_check_state(void) {
    return gCheckState;
}

const char *folio_sparkle_found_version(void) {
    return gFoundVersion != nil ? [gFoundVersion UTF8String] : NULL;
}

const char *folio_sparkle_found_build(void) {
    return gFoundBuild != nil ? [gFoundBuild UTF8String] : NULL;
}
