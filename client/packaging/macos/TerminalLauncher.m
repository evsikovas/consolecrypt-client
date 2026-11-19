#import <AppKit/AppKit.h>

int main(void) {
  @autoreleasepool {
    NSString *appIdentifier = @"io.consolecrypt.consolecrypt";
    NSURL *sibling = [NSBundle.mainBundle.bundleURL.URLByDeletingLastPathComponent
        URLByAppendingPathComponent:@"ConsoleCrypt.app" isDirectory:YES];
    NSURL *target = [[NSBundle bundleWithURL:sibling].bundleIdentifier isEqualToString:appIdentifier]
        ? sibling
        : [NSWorkspace.sharedWorkspace URLForApplicationWithBundleIdentifier:appIdentifier];

    if (target) {
      NSTask *task = [[NSTask alloc] init];
      task.executableURL = [NSURL fileURLWithPath:@"/usr/bin/open"];
      task.arguments = @[@"-n", @"-a", target.path, @"--args", @"--terminal-only"];
      if ([task launchAndReturnError:NULL]) {
        [task waitUntilExit];
        if (task.terminationStatus == 0) return EXIT_SUCCESS;
      }
    }

    BOOL russian = [NSLocale.preferredLanguages.firstObject hasPrefix:@"ru"];
    [NSApplication.sharedApplication activateIgnoringOtherApps:YES];
    NSAlert *alert = [[NSAlert alloc] init];
    alert.alertStyle = NSAlertStyleWarning;
    alert.messageText = @"ConsoleCrypt Terminal";
    alert.informativeText = russian
        ? @"Не удалось открыть терминал. Установите ConsoleCrypt.app в папку «Программы»."
        : @"Could not open the terminal. Install ConsoleCrypt.app in Applications.";
    [alert runModal];
    return EXIT_FAILURE;
  }
}
