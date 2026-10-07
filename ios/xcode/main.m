#import <UIKit/UIKit.h>
#import "tuclaw_ios.h"

@interface TuclawAppDelegate : UIResponder <UIApplicationDelegate>
@property (nonatomic, assign) void *gpuiWindow;
@property (nonatomic, strong) CADisplayLink *displayLink;
@end

static void TuclawResumeFrames(void *context) {
    TuclawAppDelegate *delegate = (__bridge TuclawAppDelegate *)context;
    delegate.displayLink.paused = NO;
}

@implementation TuclawAppDelegate

- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    tuclaw_ios_start();
    self.gpuiWindow = gpui_ios_get_window();
    [self startFrames];
    return YES;
}

- (void)startFrames {
    if (!self.gpuiWindow || self.displayLink) {
        return;
    }
    self.displayLink = [CADisplayLink displayLinkWithTarget:self selector:@selector(renderFrame)];
    [self.displayLink addToRunLoop:[NSRunLoop mainRunLoop] forMode:NSRunLoopCommonModes];
    gpui_ios_set_frame_waker(self.gpuiWindow, TuclawResumeFrames, (__bridge void *)self);
}

- (void)stopFrames {
    [self.displayLink invalidate];
    self.displayLink = nil;
}

- (void)renderFrame {
    if (!gpui_ios_request_frame(self.gpuiWindow)) {
        self.displayLink.paused = YES;
    }
}

- (void)applicationWillEnterForeground:(UIApplication *)application {
    gpui_ios_will_enter_foreground(NULL);
    [self startFrames];
}

- (void)applicationDidBecomeActive:(UIApplication *)application {
    gpui_ios_did_become_active(NULL);
}

- (void)applicationWillResignActive:(UIApplication *)application {
    gpui_ios_will_resign_active(NULL);
}

- (void)applicationDidEnterBackground:(UIApplication *)application {
    gpui_ios_did_enter_background(NULL);
    [self stopFrames];
}

- (void)applicationWillTerminate:(UIApplication *)application {
    [self stopFrames];
    gpui_ios_will_terminate(NULL);
}

- (BOOL)application:(UIApplication *)application openURL:(NSURL *)url options:(NSDictionary<UIApplicationOpenURLOptionsKey, id> *)options {
    gpui_ios_handle_open_url((__bridge void *)url.absoluteString);
    return YES;
}

@end

int main(int argc, char *argv[]) {
    @autoreleasepool {
        return UIApplicationMain(argc, argv, nil, NSStringFromClass([TuclawAppDelegate class]));
    }
}
