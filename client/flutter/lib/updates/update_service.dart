import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/app_info.dart';
import 'package:crypto/crypto.dart' as digest;
import 'package:cryptography/cryptography.dart';
import 'package:flutter/services.dart';

const updateFeedUrl = String.fromEnvironment(
  'CC_UPDATE_FEED',
  defaultValue: 'https://updates.consolecrypt.evsikov.net/stable.json',
);

/// A PUBLIC trust anchor, never a signing key. Private publisher material is
/// managed independently of this repository (ADR-0106).
const updateVerificationKey = String.fromEnvironment(
  'CC_UPDATE_PUBLIC_KEY',
  defaultValue: 'aK3R5vXwJ9eeqQ1iYah9fzBAIHOeKNejoRdI8weXUYo=',
);

class UpdateException implements Exception {
  const UpdateException(this.code);
  final String code;
  @override
  String toString() => 'UpdateException($code)';
}

final class UpdateRelease {
  const UpdateRelease({
    required this.version,
    required this.build,
    required this.platform,
    required this.url,
    required this.bytes,
    required this.sha256,
    required this.notes,
  });
  final String version;
  final int build;
  final String platform;
  final Uri url;
  final int bytes;
  final String sha256;
  final Map<String, String> notes;

  String get fileName =>
      'ConsoleCrypt-$version+$build-${switch (platform) {
        'windows-x64' => 'windows-x64-setup.exe',
        'macos-universal' => 'macos-universal.dmg',
        'android-arm64' => 'android-arm64.apk',
        _ => throw const UpdateException('platform'),
      }}';
}

List<int> _version(String value) {
  if (!RegExp(r'^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$').hasMatch(value)) {
    throw const UpdateException('version');
  }
  return value.split('.').map(int.parse).toList();
}

bool newerVersion(String candidate, String current) {
  final a = _version(candidate);
  final b = _version(current);
  for (var i = 0; i < 3; i++) {
    if (a[i] != b[i]) return a[i] > b[i];
  }
  return false;
}

bool safeUpdateUrl(Uri url) =>
    url.scheme == 'https' &&
    url.userInfo.isEmpty &&
    url.port == 443 &&
    !url.hasFragment &&
    const {'git.evsikov.net', 'updates.consolecrypt.evsikov.net'}.contains(url.host);

/// Authenticates the exact signed bytes BEFORE reading any installer URL.
Future<UpdateRelease?> verifyUpdateFeed(
  List<int> envelope, {
  required String platform,
  String currentVersion = kAppVersion,
  String publicKey = updateVerificationKey,
  DateTime? now,
}) async {
  if (envelope.length > 65536) throw const UpdateException('manifest_size');
  try {
    final wrapper = jsonDecode(utf8.decode(envelope)) as Map<String, dynamic>;
    final payload = base64Decode(wrapper['payload'] as String);
    final signatureBytes = base64Decode(wrapper['signature'] as String);
    final keyBytes = base64Decode(publicKey);
    if (keyBytes.length != 32 || signatureBytes.length != 64 || payload.length > 32768) {
      throw const UpdateException('signature');
    }
    final valid = await Ed25519().verify(
      payload,
      signature: Signature(signatureBytes, publicKey: SimplePublicKey(keyBytes, type: KeyPairType.ed25519)),
    );
    if (!valid) throw const UpdateException('signature');
    final data = jsonDecode(utf8.decode(payload)) as Map<String, dynamic>;
    final instant = (now ?? DateTime.now()).toUtc();
    final issued = DateTime.parse(data['issuedAt'] as String).toUtc();
    final expires = DateTime.parse(data['expiresAt'] as String).toUtc();
    if (data['schema'] != 1 ||
        issued.isAfter(instant.add(const Duration(minutes: 10))) ||
        !expires.isAfter(instant) ||
        expires.difference(issued) > const Duration(days: 100) ||
        !expires.isAfter(issued)) {
      throw const UpdateException('manifest_expired');
    }
    final version = data['version'] as String;
    _version(version);
    final asset = (data['platforms'] as Map<String, dynamic>)[platform];
    if (asset == null) return null;
    final item = asset as Map<String, dynamic>;
    final url = Uri.parse(item['url'] as String);
    final size = item['bytes'] as int;
    final hash = item['sha256'] as String;
    final build = item['build'] as int;
    if (!safeUpdateUrl(url) || size < 1 || size > 524288000 || build < 1 || !RegExp(r'^[a-f0-9]{64}$').hasMatch(hash)) {
      throw const UpdateException('asset');
    }
    if (!newerVersion(version, currentVersion)) return null;
    final notes = data['notes'] as Map<String, dynamic>? ?? {};
    return UpdateRelease(
      version: version,
      build: build,
      platform: platform,
      url: url,
      bytes: size,
      sha256: hash,
      notes: {
        for (final language in ['en', 'ru']) language: notes[language] as String? ?? '',
      },
    );
  } on UpdateException {
    rethrow;
  } on Object {
    throw const UpdateException('manifest_format');
  }
}

enum UpdateInstallResult { opened, exitWindows, androidPermission }

class UpdateService {
  UpdateService({HttpClient Function()? clientFactory, this.publicKey = updateVerificationKey})
    : _clientFactory = clientFactory ?? HttpClient.new;
  final HttpClient Function() _clientFactory;
  final String publicKey;
  static const _android = MethodChannel('consolecrypt/updates');

  String get platform => Platform.isWindows
      ? 'windows-x64'
      : Platform.isMacOS
      ? 'macos-universal'
      : Platform.isAndroid
      ? 'android-arm64'
      : 'unsupported';

  HttpClient _client() => _clientFactory()
    ..connectionTimeout = const Duration(seconds: 15)
    ..idleTimeout = const Duration(seconds: 30)
    ..userAgent = 'ConsoleCrypt-Updates/$kAppVersion';

  Future<HttpClientResponse> _get(HttpClient client, Uri initial) async {
    var url = initial;
    for (var count = 0; count < 4; count++) {
      if (!safeUpdateUrl(url)) throw const UpdateException('url');
      final request = await client.getUrl(url).timeout(const Duration(seconds: 20));
      request.followRedirects = false;
      final response = await request.close().timeout(const Duration(seconds: 30));
      if (const {301, 302, 303, 307, 308}.contains(response.statusCode)) {
        final next = response.headers.value(HttpHeaders.locationHeader);
        await response.drain<void>().timeout(const Duration(seconds: 10));
        if (next == null) throw const UpdateException('redirect');
        url = url.resolve(next);
        continue;
      }
      if (response.statusCode != HttpStatus.ok) throw const UpdateException('http');
      return response;
    }
    throw const UpdateException('redirect');
  }

  Future<UpdateRelease?> check() async {
    if (platform == 'unsupported') throw const UpdateException('platform');
    final client = _client();
    try {
      final response = await _get(client, Uri.parse(updateFeedUrl));
      final bytes = <int>[];
      await for (final chunk in response.timeout(const Duration(seconds: 30))) {
        bytes.addAll(chunk);
        if (bytes.length > 65536) throw const UpdateException('manifest_size');
      }
      return await verifyUpdateFeed(bytes, platform: platform, publicKey: publicKey);
    } finally {
      client.close(force: true);
    }
  }

  Future<File> download(UpdateRelease release, void Function(double) progress) async {
    final client = _client();
    RandomAccessFile? output;
    Directory? folder;
    try {
      if (Platform.isAndroid) {
        final path = await _android.invokeMethod<String>('cacheDirectory');
        if (path == null) throw const UpdateException('storage');
        folder = await Directory(path).createTemp('release-');
      } else {
        folder = await Directory.systemTemp.createTemp('consolecrypt-update-');
      }
      final file = File('${folder.path}/${release.fileName}');
      final response = await _get(client, release.url);
      if (response.contentLength != -1 && response.contentLength != release.bytes) {
        throw const UpdateException('size');
      }
      output = await file.open(mode: FileMode.write);
      var received = 0;
      await for (final chunk in response.timeout(const Duration(seconds: 45))) {
        received += chunk.length;
        if (received > release.bytes) throw const UpdateException('size');
        await output.writeFrom(chunk);
        progress(received / release.bytes);
      }
      await output.flush();
      await output.close();
      output = null;
      if (received != release.bytes) throw const UpdateException('size');
      await verifyInstaller(file, release);
      return file;
    } on Object {
      await output?.close();
      output = null;
      if (folder != null && folder.existsSync()) await folder.delete(recursive: true);
      rethrow;
    } finally {
      client.close(force: true);
    }
  }

  /// Recheck on installation as well, including after a permissions round trip.
  Future<void> verifyInstaller(File file, UpdateRelease release) async {
    if (await file.length() != release.bytes ||
        (await digest.sha256.bind(file.openRead()).first).toString() != release.sha256) {
      throw const UpdateException('checksum');
    }
  }

  Future<UpdateInstallResult> install(File file, UpdateRelease release) async {
    if (release.platform != platform) throw const UpdateException('platform');
    await verifyInstaller(file, release);
    if (Platform.isWindows) {
      await Process.start(file.path, [
        '/SILENT',
        '/CLOSEAPPLICATIONS',
        '/RESTARTAPPLICATIONS',
      ], mode: ProcessStartMode.detached);
      return UpdateInstallResult.exitWindows;
    }
    if (Platform.isMacOS) {
      final result = await Process.run('/usr/bin/open', [file.path]);
      if (result.exitCode != 0) throw const UpdateException('installer');
      return UpdateInstallResult.opened;
    }
    if (Platform.isAndroid) {
      final result = await _android.invokeMethod<String>('install', {'path': file.path, 'version': release.version});
      if (result == 'permission') return UpdateInstallResult.androidPermission;
      if (result != 'opened') throw const UpdateException('installer');
      return UpdateInstallResult.opened;
    }
    throw const UpdateException('platform');
  }
}
