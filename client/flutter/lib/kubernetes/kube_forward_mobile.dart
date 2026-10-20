import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/services.dart';

abstract final class KubeForwardMobile {
  static const _channel = MethodChannel('consolecrypt/kube_forwards');

  static Future<void> prepare() async {
    if (!AppPlatform.isAndroid) return;
    await _channel.invokeMethod<void>('prepare');
  }

  /// A native notification remains responsible for Stop even without Dart.
  static Future<void> keepVisible() async {
    if (!AppPlatform.isAndroid) return;
    await _channel.invokeMethod<void>('start');
  }
}
