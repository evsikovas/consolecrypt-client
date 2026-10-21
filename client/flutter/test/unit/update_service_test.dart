import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:crypto/crypto.dart';
import 'package:cryptography/cryptography.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
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

  test('transition accepts signed assets on the new domain and exact GitHub release path', () async {
    expect(updateFeedUrl, 'https://updates.consolecrypt.dev/stable.json');
    final asset = (payload['platforms']! as Map<String, Object?>)['windows-x64']! as Map<String, Object?>;
    for (final url in [
      'https://updates.consolecrypt.dev/releases/0.3.2/ConsoleCrypt-0.3.2+11012-windows-x64-setup.exe',
      'https://updates.consolecrypt.evsikov.net/releases/0.3.2/client.exe',
      'https://github.com/evsikovas/consolecrypt-client/releases/download/v0.3.2/ConsoleCrypt-0.3.2%2B11012-windows-x64-setup.exe',
      'https://release-assets.githubusercontent.com/github-production-release-asset/123/asset-id?signature=example',
    ]) {
      asset['url'] = url;
      expect((await verify(await envelope()))!.url, Uri.parse(url));
    }
    for (final url in [
      'https://github.com/attacker/consolecrypt-client/releases/download/v0.3.2/ConsoleCrypt-installer.exe',
      'https://github.com/evsikovas/other/releases/download/v0.3.2/ConsoleCrypt-installer.exe',
      'https://github.com/evsikovas/consolecrypt-client/archive/refs/heads/main.zip',
      'https://github.com/evsikovas/consolecrypt-client/releases/download/v0.3.2/ConsoleCrypt.exe?next=other',
      'https://github.com.evil.invalid/evsikovas/consolecrypt-client/releases/download/v0.3.2/ConsoleCrypt.exe',
      'https://release-assets.githubusercontent.com/other/file.exe',
      'https://raw.githubusercontent.com/evsikovas/consolecrypt-client/main/client.exe',
      'https://updates.consolecrypt.dev:8443/client.exe',
      'https://user@updates.consolecrypt.dev/client.exe',
      'http://updates.consolecrypt.dev/client.exe',
    ]) {
      asset['url'] = url;
      await expectLater(verify(await envelope()), throwsA(isA<UpdateException>()), reason: url);
    }
  });

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
  test('the same version with a newer build is offered, an older or equal build is not', () async {
    payload['version'] = '0.6.4';
    final asset = (payload['platforms']! as Map<String, Object?>)['windows-x64']! as Map<String, Object?>;
    asset['build'] = 29011;
    Future<UpdateRelease?> check(int build) async => verifyUpdateFeed(
      await envelope(),
      platform: 'windows-x64',
      currentVersion: '0.6.4',
      currentBuild: build,
      publicKey: publicKey,
      now: clock,
    );
    expect((await check(21029))?.build, 29011);
    expect(await check(29011), isNull);
    expect(await check(29012), isNull);
    expect(newerVersion('0.6.4', '0.7.2', candidateBuild: 99999, currentBuild: 1), isFalse);
    expect(newerVersion('0.6.5', '0.6.4', candidateBuild: 1, currentBuild: 99999), isTrue);
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
  test('failed installer retries fetch fresh bytes; Android permission retry keeps verified bytes', () async {
    final release = (await verify(await envelope()))!;
    final fake = _InstallService(release);
    final container = ProviderContainer(overrides: [updateServiceProvider.overrideWithValue(fake)]);
    addTearDown(container.dispose);
    addTearDown(() => fake.folder.delete(recursive: true));
    final controller = container.read(updateControllerProvider.notifier);
    await controller.check();
    fake.failNext = true;
    await controller.downloadAndInstall();
    expect(container.read(updateControllerProvider).phase, UpdatePhase.failed);
    expect(fake.downloads, 1);
    fake.permissionNext = true;
    await controller.downloadAndInstall();
    expect(fake.downloads, 2);
    expect(container.read(updateControllerProvider).phase, UpdatePhase.permission);
    await controller.downloadAndInstall();
    expect(fake.downloads, 2);
    expect(fake.installs, 3);
    expect(container.read(updateControllerProvider).phase, UpdatePhase.opened);
  });
  test('cancelling the macOS save panel preserves a verified download for retry', () async {
    final release = (await verify(await envelope()))!;
    final fake = _InstallService(release)..cancelNext = true;
    final container = ProviderContainer(overrides: [updateServiceProvider.overrideWithValue(fake)]);
    addTearDown(container.dispose);
    addTearDown(() => fake.folder.delete(recursive: true));
    final controller = container.read(updateControllerProvider.notifier);
    await controller.check();
    await controller.downloadAndInstall();
    expect(container.read(updateControllerProvider).phase, UpdatePhase.ready);
    expect(container.read(updateControllerProvider).error, isNull);
    await controller.downloadAndInstall();
    expect(fake.downloads, 1);
    expect(fake.installs, 2);
    expect(container.read(updateControllerProvider).phase, UpdatePhase.opened);
  });
  test('macOS hands only verified DMG bytes to the native consent flow', () async {
    if (!Platform.isMacOS) return;
    const channel = MethodChannel('test/macos-update-consent');
    final calls = <MethodCall>[];
    var outcome = 'opened';
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      return outcome;
    });
    addTearDown(() => messenger.setMockMethodCallHandler(channel, null));
    final folder = await Directory.systemTemp.createTemp('consolecrypt-update-test-');
    addTearDown(() => folder.delete(recursive: true));
    final release = UpdateRelease(
      version: '0.2.2',
      build: 1400,
      platform: 'macos-universal',
      url: Uri.parse('https://git.evsikov.net/releases/client.dmg'),
      bytes: 3,
      sha256: sha256.convert([1, 2, 3]).toString(),
      notes: const {},
    );
    final file = File('${folder.path}/${release.fileName}');
    await file.writeAsBytes([1, 2, 3]);
    final service = UpdateService(macosChannel: channel);
    expect(
      await service.install(file, release, macosSaveTitle: 'Save verified update', macosSavePrompt: 'Save'),
      UpdateInstallResult.opened,
    );
    expect(calls.single.method, 'saveAndOpen');
    expect(calls.single.arguments, {
      'path': file.path,
      'fileName': release.fileName,
      'bytes': 3,
      'sha256': release.sha256,
      'title': 'Save verified update',
      'prompt': 'Save',
    });
    outcome = 'cancelled';
    expect(await service.install(file, release), UpdateInstallResult.cancelled);
    outcome = 'destination_exists';
    await expectLater(
      service.install(file, release),
      throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'destination_exists')),
    );
    final before = calls.length;
    await file.writeAsBytes([3, 2, 1]);
    await expectLater(service.install(file, release), throwsA(isA<UpdateException>()));
    expect(calls.length, before);
  });
  group('Windows in-place install', () {
    late Directory folder;
    late UpdateRelease release;
    late File installer;
    setUp(() async {
      folder = await Directory.systemTemp.createTemp('consolecrypt-windows-update-test-');
      release = UpdateRelease(
        version: '0.8.6',
        build: 33000,
        platform: 'windows-x64',
        url: Uri.parse('https://updates.consolecrypt.dev/releases/0.8.6/setup.exe'),
        bytes: 3,
        sha256: sha256.convert([1, 2, 3]).toString(),
        notes: const {},
      );
      installer = File('${folder.path}/${release.fileName}');
      await installer.writeAsBytes([1, 2, 3]);
    });
    tearDown(() => folder.delete(recursive: true));

    UpdateService windows({
      required List<(String, List<String>)> launches,
      bool installed = true,
      Exception? failure,
    }) => UpdateService(
      platform: 'windows-x64',
      processId: 4242,
      windowsInstalled: () async => installed,
      launcher: (executable, arguments) async {
        if (failure != null) throw failure;
        // The verified bytes are still in place while Setup starts.
        expect(File(executable).readAsBytesSync(), [1, 2, 3]);
        launches.add((executable, arguments));
      },
    );

    test('verifies the bytes right before launch and asks Setup to wait for this process', () async {
      final launches = <(String, List<String>)>[];
      final service = windows(launches: launches);
      expect(await service.install(installer, release), UpdateInstallResult.exitWindows);
      expect(launches.single.$1, installer.absolute.path);
      expect(launches.single.$2, [
        '/SP-',
        '/SILENT',
        '/NORESTART',
        '/CLOSEAPPLICATIONS',
        '/RELAUNCH=1',
        '/WAITPID=4242',
      ]);
    });

    test('a file changed after download is never launched', () async {
      final launches = <(String, List<String>)>[];
      final service = windows(launches: launches);
      for (final bytes in <List<int>>[
        [3, 2, 1],
        [1, 2],
        [1, 2, 3, 4],
      ]) {
        await installer.writeAsBytes(bytes);
        await expectLater(
          service.install(installer, release),
          throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'checksum')),
        );
      }
      await installer.delete();
      await expectLater(service.install(installer, release), throwsA(isA<UpdateException>()));
      expect(launches, isEmpty);
    });

    test('a failed launch is reported with the OS error number', () async {
      final service = windows(launches: [], failure: const ProcessException('setup.exe', [], 'blocked', 225));
      await expectLater(
        service.install(installer, release),
        throwsA(
          isA<UpdateException>().having((e) => e.code, 'code', 'launch').having((e) => e.detail, 'detail', '225'),
        ),
      );
      // The file is released after a failure (a retry downloads fresh bytes).
      await installer.delete();
    });

    test('a portable ZIP copy is never handed to Setup', () async {
      final launches = <(String, List<String>)>[];
      final service = windows(launches: launches, installed: false);
      expect(await service.canInstallInPlace(), isFalse);
      await expectLater(
        service.install(installer, release),
        throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'portable')),
      );
      expect(launches, isEmpty);
      expect(
        await UpdateService(platform: 'macos-universal', windowsInstalled: () async => false).canInstallInPlace(),
        isTrue,
      );
    });
  });

  test('installed and portable Windows copies are told apart by the Inno Setup uninstaller', () {
    expect(windowsInstalledByInstaller(['ConsoleCrypt.exe', 'unins000.exe', 'unins000.dat']), isTrue);
    expect(windowsInstalledByInstaller(['ConsoleCrypt.exe', 'UNINS001.EXE']), isTrue);
    expect(windowsInstalledByInstaller(['ConsoleCrypt.exe', 'flutter_windows.dll', 'data']), isFalse);
    expect(windowsInstalledByInstaller(['ConsoleCrypt.exe', 'unins000.dat']), isFalse);
    expect(windowsInstalledByInstaller(['uninstall.exe']), isFalse);
  });

  test('verbatim Windows paths are simplified for Setup and display', () {
    expect(
      withoutVerbatimPrefix(r'\\?\C:\Users\me\AppData\Local\Temp\x\setup.exe'),
      r'C:\Users\me\AppData\Local\Temp\x\setup.exe',
    );
    expect(withoutVerbatimPrefix(r'\\?\UNC\server\share\setup.exe'), r'\\server\share\setup.exe');
    expect(withoutVerbatimPrefix(r'C:\ProgramData\chocolatey\bin\flux.exe'), r'C:\ProgramData\chocolatey\bin\flux.exe');
    expect(withoutVerbatimPrefix(r'\\server\share\a'), r'\\server\share\a');
    expect(withoutVerbatimPrefix(r'\\?\GLOBALROOT\Device\x'), r'\\?\GLOBALROOT\Device\x');
    expect(withoutVerbatimPrefix('/tmp/consolecrypt-update-x/setup.exe'), '/tmp/consolecrypt-update-x/setup.exe');
  });

  test('an older signed feed than the last accepted one is refused (no update freeze)', () async {
    final issued = clock;
    expect(
      await verifyUpdateFeed(
        await envelope(),
        platform: 'windows-x64',
        currentVersion: '0.1.10',
        publicKey: publicKey,
        now: clock,
        notBefore: issued,
      ),
      isNotNull,
    );
    DateTime? seen;
    await verifyUpdateFeed(
      await envelope(),
      platform: 'windows-x64',
      currentVersion: '0.1.10',
      publicKey: publicKey,
      now: clock,
      onAuthenticated: (value) => seen = value,
    );
    expect(seen, issued);
    await expectLater(
      verifyUpdateFeed(
        await envelope(),
        platform: 'windows-x64',
        currentVersion: '0.1.10',
        publicKey: publicKey,
        now: clock,
        notBefore: issued.add(const Duration(seconds: 1)),
      ),
      throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'manifest_rollback')),
    );
  });

  test('check persists the newest feed time and rejects a replayed older feed', () async {
    final now = DateTime.now().toUtc();
    payload['version'] = '99.0.0';
    Future<List<int>> feedIssued(DateTime issued) async {
      payload['issuedAt'] = issued.toIso8601String();
      payload['expiresAt'] = issued.add(const Duration(days: 90)).toIso8601String();
      return envelope();
    }

    final store = _HighWater();
    Future<UpdateRelease?> checkWith(List<int> body) => UpdateService(
      clientFactory: () => _Client(_Response([body])),
      publicKey: publicKey,
      platform: 'windows-x64',
      feedHighWater: store,
    ).check();

    final newer = await feedIssued(now.subtract(const Duration(hours: 1)));
    final older = await feedIssued(now.subtract(const Duration(days: 3)));
    expect(await checkWith(newer), isNotNull);
    expect(store.value, now.subtract(const Duration(hours: 1)));
    expect(store.writes, 1);
    expect(await checkWith(newer), isNotNull, reason: 'the same feed again is fine');
    expect(store.writes, 1);
    await expectLater(
      checkWith(older),
      throwsA(isA<UpdateException>().having((e) => e.code, 'code', 'manifest_rollback')),
    );
    expect(store.value, now.subtract(const Duration(hours: 1)));
    // A corrupt value in the future does not block updates forever.
    store.value = now.add(const Duration(days: 365));
    expect(await checkWith(older), isNotNull);
    expect(store.value, now.subtract(const Duration(days: 3)));
  });

  test('the accepted feed time survives settings serialization', () {
    final issued = DateTime.utc(2026, 10, 21, 12, 32, 16, 322);
    final reopened = localSettingsFromJson(
      localSettingsToJson(const LocalSettings().copyWith(updateFeedIssuedAt: issued)),
    );
    expect(reopened.updateFeedIssuedAt, issued);
    expect(localSettingsFromJson({}).updateFeedIssuedAt, isNull);
    expect(localSettingsFromJson({'update_feed_issued_at': 'garbage'}).updateFeedIssuedAt, isNull);
    expect(reopened.copyWith(terminalFontSize: 16).updateFeedIssuedAt, issued);
  });

  test(
    'controller: portable copies never download, launch failures keep their detail, Windows exits for Setup',
    () async {
      final release = UpdateRelease(
        version: '0.8.6',
        build: 33000,
        platform: 'windows-x64',
        url: Uri.parse('https://updates.consolecrypt.dev/releases/0.8.6/setup.exe'),
        bytes: 3,
        sha256: sha256.convert([1, 2, 3]).toString(),
        notes: const {},
      );
      final fake = _InstallService(release)..inPlace = false;
      var exits = 0;
      final container = ProviderContainer(
        overrides: [
          updateServiceProvider.overrideWithValue(fake),
          updateAppExitProvider.overrideWithValue(() async => exits++),
        ],
      );
      addTearDown(container.dispose);
      addTearDown(() => fake.folder.delete(recursive: true));
      final controller = container.read(updateControllerProvider.notifier);
      await controller.check();
      expect(container.read(updateControllerProvider).inPlace, isFalse);
      await controller.downloadAndInstall();
      expect(fake.downloads, 0);
      expect(container.read(updateControllerProvider).error, 'portable');

      fake.inPlace = true;
      await controller.check();
      fake.failure = const UpdateException('launch', '5');
      await controller.downloadAndInstall();
      final failed = container.read(updateControllerProvider);
      expect((failed.phase, failed.error, failed.errorDetail), (UpdatePhase.failed, 'launch', '5'));
      expect(exits, 0);

      fake.result = UpdateInstallResult.exitWindows;
      await controller.downloadAndInstall();
      expect(container.read(updateControllerProvider).phase, UpdatePhase.opened);
      expect(exits, 1);
    },
  );
}

class _HighWater implements UpdateFeedHighWater {
  DateTime? value;
  int writes = 0;
  @override
  Future<DateTime?> read() async => value;
  @override
  Future<void> write(DateTime issuedAt) async {
    writes++;
    value = issuedAt;
  }
}

class _InstallService extends UpdateService {
  _InstallService(this.release);
  final UpdateRelease release;
  final Directory folder = Directory.systemTemp.createTempSync('consolecrypt-installer-test-');
  int downloads = 0;
  int installs = 0;
  bool failNext = false;
  bool permissionNext = false;
  bool cancelNext = false;
  bool inPlace = true;
  UpdateException? failure;
  UpdateInstallResult result = UpdateInstallResult.opened;
  @override
  Future<UpdateRelease?> check() async => release;
  @override
  Future<bool> canInstallInPlace() async => inPlace;
  @override
  Future<File> download(UpdateRelease release, void Function(double) progress) async {
    downloads++;
    final file = File('${folder.path}/${release.fileName}');
    await file.writeAsBytes([1, 2, 3]);
    progress(1);
    return file;
  }

  @override
  Future<UpdateInstallResult> install(
    File file,
    UpdateRelease release, {
    String? macosSaveTitle,
    String? macosSavePrompt,
  }) async {
    installs++;
    await verifyInstaller(file, release);
    if (failNext) {
      failNext = false;
      throw const UpdateException('installer');
    }
    if (cancelNext) {
      cancelNext = false;
      return UpdateInstallResult.cancelled;
    }
    if (permissionNext) {
      permissionNext = false;
      return UpdateInstallResult.androidPermission;
    }
    if (failure case final error?) {
      failure = null;
      throw error;
    }
    return result;
  }
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
