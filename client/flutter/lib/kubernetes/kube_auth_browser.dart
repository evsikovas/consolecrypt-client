import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/platform/system_browser.dart';
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
    await openInSystemBrowser(url);
    return null;
  }

  @override
  Future<void> cancel(String ticket) async {
    if (AppPlatform.isMobile) await _channel.invokeMethod<void>('cancelBrowser', {'ticket': ticket});
  }
}
