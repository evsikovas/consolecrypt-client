import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:crypto/crypto.dart';
import 'package:cryptography/cryptography.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final clock = DateTime.utc(2026, 9, 30);
  late SimpleKeyPair key;
  late String publicKey;
  late Map<String, Object?> payload;
  setUp(() async {
    key = await Ed25519().newKeyPair();
    publicKey = base64Encode((await key.extractPublicKey()).bytes);
    payload = {
      'schema': 1,
      'issuedAt': clock.toIso8601String(),
      'expiresAt': clock.add(const Duration(days: 90)).toIso8601String(),
      'version': '0.2.0',
      'notes': {'en': 'Keyboard fix', 'ru': 'Исправлен ввод'},
      'platforms': {
        'windows-x64': {
          'build': 1300,
          'url': 'https://git.evsikov.net/releases/client.exe',
          'bytes': 3,
          'sha256': sha256.convert([1, 2, 3]).toString(),
        },
      },
    };
  });
  Future<List<int>> envelope() async {
    final bytes = utf8.encode(jsonEncode(payload));
    final signature = await Ed25519().sign(bytes, keyPair: key);
    return utf8.encode(jsonEncode({'payload': base64Encode(bytes), 'signature': base64Encode(signature.bytes)}));
  }

  Future<UpdateRelease?> verify(List<int> bytes) =>
      verifyUpdateFeed(bytes, platform: 'windows-x64', currentVersion: '0.1.10', publicKey: publicKey, now: clock);

  test('authenticates a runtime-generated signature and selects the Windows installer', () async {
    final release = await verify(await envelope());
    expect(release!.version, '0.2.0');
    expect(release.fileName, 'ConsoleCrypt-0.2.0+1300-windows-x64-setup.exe');
    expect(release.notes['ru'], 'Исправлен ввод');
  });
  test('rejects a modified payload, signature and untrusted signing key', () async {
    final bytes = await envelope();
    final wrapper = jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
    wrapper['payload'] = base64Encode(utf8.encode('{}'));
    await expectLater(verify(utf8.encode(jsonEncode(wrapper))), throwsA(isA<UpdateException>()));
    wrapper['signature'] = base64Encode(List<int>.filled(64, 0));
    await expectLater(verify(utf8.encode(jsonEncode(wrapper))), throwsA(isA<UpdateException>()));
    final other = await Ed25519().newKeyPair();
    await expectLater(
      verifyUpdateFeed(
        bytes,
        platform: 'windows-x64',
        publicKey: base64Encode((await other.extractPublicKey()).bytes),
        now: clock,
      ),
      throwsA(isA<UpdateException>()),
    );
  });
  test('refuses expired or future manifests and never offers a downgrade', () async {
    payload['expiresAt'] = clock.toIso8601String();
    await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()));
    payload['expiresAt'] = clock.add(const Duration(days: 90)).toIso8601String();
    payload['issuedAt'] = clock.add(const Duration(days: 1)).toIso8601String();
    await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()));
    payload['issuedAt'] = clock.toIso8601String();
    for (final version in ['0.1.9', '0.1.10']) {
      payload['version'] = version;
      expect(await verify(await envelope()), isNull);
    }
    payload['version'] = '../installer';
    await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()));
  });
  test('rejects unsigned hostile URLs and unreasonable installer sizes', () async {
    final asset = (payload['platforms']! as Map<String, Object?>)['windows-x64']! as Map<String, Object?>;
    for (final url in [
      'http://git.evsikov.net/a',
      'https://attacker.invalid/a',
      'https://password@git.evsikov.net/a',
      'https://git.evsikov.net:444/a',
    ]) {
      asset['url'] = url;
      await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()));
    }
    asset['url'] = 'https://git.evsikov.net/a';
    asset['bytes'] = 1000000000;
    await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()));
    await expectLater(verify(List<int>.filled(65537, 0)), throwsA(isA<UpdateException>()));
    expect(
      await verifyUpdateFeed(await envelope(), platform: 'android-arm64', publicKey: publicKey, now: clock),
      isNull,
    );
  });
  test('automatic check choice survives serialization, restart and unrelated settings changes', () {
    expect(localSettingsFromJson({}).checkUpdatesAutomatically, isTrue);
    final disabled = const LocalSettings().copyWith(checkUpdatesAutomatically: false);
    final reopened = localSettingsFromJson(localSettingsToJson(disabled));
    expect(reopened.checkUpdatesAutomatically, isFalse);
    expect(reopened.copyWith(terminalFontSize: 16).checkUpdatesAutomatically, isFalse);
  });
  test('streams an installer, validates its hash and rechecks it before installation', () async {
    final client = _Client(
      _Response([
        [1],
        [2, 3],
      ], bodyLength: 3),
    );
    final service = UpdateService(clientFactory: () => client);
    final release = (await verify(await envelope()))!;
    final progress = <double>[];
    final file = await service.download(release, progress.add);
    addTearDown(() => file.parent.delete(recursive: true));
    expect(await file.readAsBytes(), [1, 2, 3]);
    expect(progress.last, 1);
    expect(client.closed, isTrue);
    await file.writeAsBytes([3, 2, 1]);
    await expectLater(service.verifyInstaller(file, release), throwsA(isA<UpdateException>()));
  });
  test('refuses a truncated or oversized download, corrupt digest and hostile redirect', () async {
    final release = (await verify(await envelope()))!;
    for (final body in <List<int>>[
      [1, 2],
      [1, 2, 3, 4],
      [3, 2, 1],
    ]) {
      final client = _Client(_Response([body]));
      await expectLater(
        UpdateService(clientFactory: () => client).download(release, (_) {}),
        throwsA(isA<UpdateException>()),
      );
      expect(client.closed, isTrue);
    }
    final redirect = _Client(_Response([], status: 302, location: 'https://attacker.invalid/file.exe'));
    await expectLater(
      UpdateService(clientFactory: () => redirect).download(release, (_) {}),
      throwsA(isA<UpdateException>()),
    );
    expect(redirect.requests, 1);
  });
  test('coalesces simultaneous manual checks and preserves manual retry after a network error', () async {
    final fake = _CheckService();
    final container = ProviderContainer(overrides: [updateServiceProvider.overrideWithValue(fake)]);
    addTearDown(container.dispose);
    final controller = container.read(updateControllerProvider.notifier);
    final first = controller.check();
    await controller.check();
    expect(fake.calls, 1);
    fake.pending.completeError(const SocketException('offline'));
    await first;
    expect(container.read(updateControllerProvider).phase, UpdatePhase.failed);
    fake.pending = Completer<UpdateRelease?>();
    final retry = controller.check();
    fake.pending.complete(null);
    await retry;
    expect(container.read(updateControllerProvider).phase, UpdatePhase.current);
  });
}

class _CheckService extends UpdateService {
  int calls = 0;
  Completer<UpdateRelease?> pending = Completer<UpdateRelease?>();
  @override
  Future<UpdateRelease?> check() {
    calls++;
    return pending.future;
  }
}

class _Client implements HttpClient {
  _Client(this.response);
  final _Response response;
  int requests = 0;
  bool closed = false;
  @override
  Future<HttpClientRequest> getUrl(Uri url) async {
    requests++;
    return _Request(response);
  }

  @override
  void close({bool force = false}) {
    closed = true;
  }

  @override
  set connectionTimeout(Duration? value) {}
  @override
  set idleTimeout(Duration value) {}
  @override
  set userAgent(String? value) {}
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

class _Request implements HttpClientRequest {
  _Request(this.response);
  final _Response response;
  @override
  set followRedirects(bool value) {}
  @override
  Future<HttpClientResponse> close() async => response;
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

class _Headers implements HttpHeaders {
  _Headers(this.location);
  final String? location;
  @override
  String? value(String name) => name == HttpHeaders.locationHeader ? location : null;
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

class _Response extends Stream<List<int>> implements HttpClientResponse {
  _Response(this.chunks, {this.bodyLength = -1, this.status = 200, this.location});
  final List<List<int>> chunks;
  final int bodyLength;
  final int status;
  final String? location;
  @override
  int get statusCode => status;
  @override
  int get contentLength => bodyLength;
  @override
  HttpHeaders get headers => _Headers(location);
  @override
  StreamSubscription<List<int>> listen(
    void Function(List<int>)? onData, {
    Function? onError,
    void Function()? onDone,
    bool? cancelOnError,
  }) =>
      Stream<List<int>>.fromIterable(chunks)
          .listen(onData, onError: onError, onDone: onDone, cancelOnError: cancelOnError);
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}
