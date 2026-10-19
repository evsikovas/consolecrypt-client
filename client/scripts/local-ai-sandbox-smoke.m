#import <Foundation/Foundation.h>
#import <arpa/inet.h>
#import <netinet/in.h>
#import <signal.h>
#import <sys/socket.h>
#import <unistd.h>

static NSMutableString *report;
static NSString *reportPath;
static volatile sig_atomic_t activeChild;

static void deadlineExpired(int signalNumber) {
    (void)signalNumber;
    if (activeChild > 0) kill(activeChild, SIGKILL);
    _exit(124);
}

static void record(NSString *line) {
    [report appendFormat:@"%@\n", line];
    [report writeToFile:reportPath atomically:YES encoding:NSUTF8StringEncoding error:NULL];
}

static void stop(NSTask *task) {
    if (task.running) [task terminate];
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:3];
    while (task.running && deadline.timeIntervalSinceNow > 0) usleep(10000);
    if (task.running) kill(task.processIdentifier, SIGKILL);
    [task waitUntilExit];
    activeChild = 0;
}

static NSTask *launch(NSString *executable, NSArray *arguments, NSString *token, NSString *log) {
    NSTask *task = [NSTask new];
    task.executableURL = [NSURL fileURLWithPath:executable];
    task.currentDirectoryURL = [NSURL fileURLWithPath:executable.stringByDeletingLastPathComponent];
    task.arguments = arguments;
    task.environment = @{@"HOME": NSHomeDirectory(), @"PATH": @"/usr/bin:/bin", @"LLAMA_API_KEY": token};
    [NSFileManager.defaultManager createFileAtPath:log contents:nil attributes:nil];
    task.standardOutput = [NSFileHandle fileHandleForWritingAtPath:log];
    task.standardError = task.standardOutput;
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        record([@"LAUNCH_FAIL " stringByAppendingString:error.description]);
        return nil;
    }
    activeChild = task.processIdentifier;
    return task;
}

static BOOL command(NSString *executable, NSString *argument) {
    NSString *log = [NSHomeDirectory() stringByAppendingPathComponent:@"local-ai-command.log"];
    NSTask *task = launch(executable, @[argument], @"unused", log);
    if (!task) return NO;
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:30];
    while (task.running && deadline.timeIntervalSinceNow > 0) usleep(10000);
    BOOL timedOut = task.running;
    if (timedOut) stop(task); else [task waitUntilExit];
    activeChild = 0;
    record([argument stringByAppendingFormat:@" %@", [NSString stringWithContentsOfFile:log encoding:NSUTF8StringEncoding error:NULL]]);
    return !timedOut && task.terminationStatus == 0;
}

static NSString *http(int port, NSString *method, NSString *path, NSString *token, NSString *body) {
    int descriptor = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons(port);
    struct timeval timeout = {2, 0};
    setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
    if (connect(descriptor, (void *)&address, sizeof(address)) != 0) { close(descriptor); return @""; }
    NSData *payload = [(body ?: @"") dataUsingEncoding:NSUTF8StringEncoding];
    NSString *request = [NSString stringWithFormat:
        @"%@ %@ HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer %@\r\nContent-Type: application/json\r\nContent-Length: %lu\r\nConnection: close\r\n\r\n%@",
        method, path, token, (unsigned long)payload.length, body ?: @""];
    const char *pointer = request.UTF8String;
    size_t remaining = strlen(pointer);
    while (remaining) {
        ssize_t count = send(descriptor, pointer, remaining, 0);
        if (count <= 0) break;
        pointer += count; remaining -= count;
    }
    NSMutableData *result = [NSMutableData data];
    char buffer[4096]; ssize_t count;
    while ((count = recv(descriptor, buffer, sizeof(buffer), 0)) > 0) {
        [result appendBytes:buffer length:count];
        if (result.length > 65536) break;
    }
    close(descriptor);
    return [[NSString alloc] initWithData:result encoding:NSUTF8StringEncoding] ?: @"";
}

int main(void) { @autoreleasepool {
    signal(SIGPIPE, SIG_IGN);
    signal(SIGALRM, deadlineExpired);
    alarm(150); // Bound model-copy/child work before the Python launch timeout.
    NSString *home = NSHomeDirectory();
    report = [NSMutableString string];
    reportPath = [home stringByAppendingPathComponent:@"local-ai-smoke.txt"];
    record([@"NSHOME " stringByAppendingString:home]);
    record([NSString stringWithFormat:@"ENV_HOME %s", getenv("HOME") ?: "(null)"]);
    if (![home containsString:@"/Library/Containers/io.consolecrypt.localai.smoke."] ||
        !getenv("HOME") || ![home isEqualToString:@(getenv("HOME"))]) {
        record(@"FAIL: expected an isolated App Sandbox container"); return 1;
    }
    NSBundle *bundle = NSBundle.mainBundle;
    NSString *architecture = bundle.infoDictionary[@"LocalAiSmokePlatform"];
    NSString *executable = [bundle.bundlePath stringByAppendingPathComponent:
        [NSString stringWithFormat:@"Contents/Helpers/LocalAI/%@/llama-server", architecture]];
    if (!command(executable, @"--version") || !command(executable, @"--list-devices")) return 2;
    if (![bundle.infoDictionary[@"LocalAiSmokeModel"] boolValue]) { record(@"RESULT PASS"); return 0; }

    NSString *model = [home stringByAppendingPathComponent:@"model.gguf"];
    NSError *error = nil;
    if (![NSFileManager.defaultManager copyItemAtPath:[bundle.resourcePath stringByAppendingPathComponent:@"model.gguf"]
                                           toPath:model error:&error]) {
        record([@"MODEL_COPY_FAIL " stringByAppendingString:error.description]); return 3;
    }
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in address = {0};
    address.sin_family = AF_INET; address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (bind(listener, (void *)&address, sizeof(address)) != 0) { close(listener); record(@"BIND_FAIL"); return 4; }
    socklen_t length = sizeof(address);
    getsockname(listener, (void *)&address, &length);
    int port = ntohs(address.sin_port); close(listener);
    NSString *token = [[NSUUID UUID].UUIDString stringByAppendingString:[NSUUID UUID].UUIDString];
    NSString *log = [home stringByAppendingPathComponent:@"local-ai-server.log"];
    NSTask *task = launch(executable, @[
        @"--model", model, @"--alias", @"sandbox-smoke", @"--host", @"127.0.0.1", @"--port", @(port).stringValue,
        @"--ctx-size", @"4096", @"--parallel", @"1", @"--n-gpu-layers", [architecture isEqualToString:@"macArm64"] ? @"99" : @"0",
        @"--offline", @"--no-webui", @"--no-slots", @"--log-disable"], token, log);
    if (!task) return 5;
    BOOL ready = NO;
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:60];
    while (task.running && deadline.timeIntervalSinceNow > 0) {
        NSString *response = http(port, @"GET", @"/health", token, nil);
        if ([response containsString:@"200 OK"] && [response containsString:@"ok"]) { ready = YES; break; }
        usleep(200000);
    }
    BOOL success = NO;
    if (ready) {
        NSString *unauthorized = http(port, @"GET", @"/v1/models", @"wrong-token", nil);
        NSString *response = http(port, @"POST", @"/v1/chat/completions", token,
            @"{\"model\":\"sandbox-smoke\",\"messages\":[{\"role\":\"user\",\"content\":\"Reply exactly OK.\"}],\"max_tokens\":12,\"temperature\":0}");
        record([@"CHAT " stringByAppendingString:response]);
        NSRange separator = [response rangeOfString:@"\r\n\r\n"];
        if (separator.location != NSNotFound) {
            NSData *body = [[response substringFromIndex:NSMaxRange(separator)] dataUsingEncoding:NSUTF8StringEncoding];
            NSDictionary *payload = [NSJSONSerialization JSONObjectWithData:body options:0 error:NULL];
            if ([payload isKindOfClass:NSDictionary.class]) {
                NSArray *choices = payload[@"choices"];
                if ([choices isKindOfClass:NSArray.class] && choices.count > 0 && [choices[0] isKindOfClass:NSDictionary.class]) {
                    NSDictionary *message = choices[0][@"message"];
                    NSString *content = [message isKindOfClass:NSDictionary.class] ? message[@"content"] : nil;
                    success = [unauthorized containsString:@"401 Unauthorized"] && [response containsString:@"200 OK"] &&
                        [content isKindOfClass:NSString.class] && [content stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet].length > 0;
                }
            }
        }
    } else {
        record(@"HEALTH_FAIL");
        record([NSString stringWithContentsOfFile:log encoding:NSUTF8StringEncoding error:NULL] ?: @"");
    }
    stop(task);
    record(success ? @"RESULT PASS" : @"RESULT FAIL");
    return success ? 0 : 6;
} }
