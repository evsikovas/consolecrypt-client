import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';

/// Opens [url] in the desktop's default browser. Never invokes a shell or
/// interpolates the URL into command text. Throws when no browser starts.
Future<void> openInSystemBrowser(Uri url) async {
  final String executable;
  final List<String> arguments;
  if (AppPlatform.isMacOS) {
    executable = '/usr/bin/open';
    arguments = ['--', url.toString()];
  } else if (AppPlatform.isWindows) {
    final windows = Platform.environment['SystemRoot'] ?? r'C:\Windows';
    executable = '$windows\\System32\\rundll32.exe';
    arguments = ['url.dll,FileProtocolHandler', url.toString()];
  } else {
    executable = 'xdg-open';
    arguments = [url.toString()];
  }
  final process = await Process.start(executable, arguments, runInShell: false);
  unawaited(process.stdout.drain<void>());
  unawaited(process.stderr.drain<void>());
  try {
    if (await process.exitCode.timeout(const Duration(seconds: 5)) != 0) {
      throw const FileSystemException('Browser unavailable'); // l10n-ignore: diagnostic
    }
  } on TimeoutException {
    // Some browser launchers remain attached to the browser process. Native
    // callback expiry/cancellation still bounds the authentication flow.
  }
}
