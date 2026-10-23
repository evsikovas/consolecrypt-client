import 'dart:async';

import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

/// The native side of `consolecrypt/clipboard` plus Flutter's platform
/// clipboard, as one simulated system clipboard. Ownership tokens follow the
/// desktop/iOS model: the change count after our write.
final class FakeNativeClipboard {
  FakeNativeClipboard({this._text});

  String? _text;
  int _changes = 0;

  /// Native calls in order (method names).
  final calls = <String>[];

  /// Arguments of the last `copySecret`.
  Map<Object?, Object?>? lastSecret;

  /// Reads of the clipboard content from Dart (must stay 0 for secrets).
  int dartReads = 0;

  /// Set to make the next native copies fail.
  bool failCopies = false;

  /// Holds native copies until completed.
  Completer<void>? gate;

  /// Runs right after a native copy wrote the clipboard (another app racing).
  void Function()? afterCopy;

  String? get text => _text;

  /// Another app (or the user) puts [value] on the clipboard.
  void userCopies(String value) {
    _text = value;
    _changes++;
  }

  /// The system clears the clipboard (e.g. iOS expiry).
  void systemClears() {
    _text = null;
    _changes++;
  }

  void install() {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(SecureClipboard.channel, _native);
    messenger.setMockMethodCallHandler(SystemChannels.platform, _platform);
    addTearDown(() {
      messenger.setMockMethodCallHandler(SecureClipboard.channel, null);
      messenger.setMockMethodCallHandler(SystemChannels.platform, null);
    });
  }

  Future<Object?> _native(MethodCall call) async {
    calls.add(call.method);
    final args = (call.arguments as Map?) ?? const {};
    switch (call.method) {
      case 'copySecret':
      case 'copyPrivate':
        await gate?.future;
        if (failCopies) throw PlatformException(code: 'clipboard');
        if (call.method == 'copySecret') lastSecret = args;
        userCopies(args['text']! as String);
        final token = 'c$_changes';
        afterCopy?.call();
        return call.method == 'copySecret' ? token : null;
      case 'clearIfOwned':
        if (args['token'] == 'c$_changes') {
          systemClears();
          return 'cleared';
        }
        return _text == null ? 'gone' : 'foreign';
      case 'forget':
        return null;
    }
    throw MissingPluginException();
  }

  Future<Object?> _platform(MethodCall call) async {
    if (call.method == 'Clipboard.setData') userCopies((call.arguments as Map)['text'] as String);
    if (call.method == 'Clipboard.getData') {
      dartReads++;
      return {'text': _text};
    }
    return null;
  }
}
