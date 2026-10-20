import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

abstract interface class KubeAuthBrowser {
  /// Desktop returns null after opening the system browser; Rust owns its
  /// loopback callback. Mobile returns the OS authentication-session callback.
  Future<String?> open(KubeOidcChallenge challenge);
  Future<void> cancel(String ticket);
}

final kubeAuthBrowserProvider = Provider<KubeAuthBrowser>((ref) => const SystemKubeAuthBrowser());

final class SystemKubeAuthBrowser implements KubeAuthBrowser {
  const SystemKubeAuthBrowser();
  static const _channel = MethodChannel('consolecrypt/kube_auth');

  @override
  Future<String?> open(KubeOidcChallenge challenge) async {
    final url = challenge.authorizationUrl;
    if (url.scheme != 'https' || url.host.isEmpty || url.userInfo.isNotEmpty || url.hasFragment) {
      throw const FormatException('Invalid authentication URL'); // l10n-ignore: diagnostic
    }
    if (AppPlatform.isMobile) {
      return _channel.invokeMethod<String>('openBrowser', {
        'url': url.toString(),
        'redirectUri': challenge.redirectUri.toString(),
        'ticket': challenge.ticket,
      });
    }
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
    // Never invoke a shell or interpolate the URL into command text.
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
    return null;
  }

  @override
  Future<void> cancel(String ticket) async {
    if (AppPlatform.isMobile) await _channel.invokeMethod<void>('cancelBrowser', {'ticket': ticket});
  }
}
