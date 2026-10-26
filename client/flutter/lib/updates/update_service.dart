import 'dart:async';
import 'dart:convert';
import 'dart:ffi' show Abi;
import 'dart:io';

import 'package:consolecrypt/app/app_info.dart';
import 'package:crypto/crypto.dart' as digest;
import 'package:cryptography/cryptography.dart';
import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:flutter/services.dart';

const updateFeedUrl = String.fromEnvironment(
  'CC_UPDATE_FEED',
  defaultValue: 'https://updates.consolecrypt.dev/stable.json',
);

/// A PUBLIC trust anchor, never a signing key. Private publisher material is
/// managed independently of this repository (ADR-0106).
const updateVerificationKey = String.fromEnvironment(
  'CC_UPDATE_PUBLIC_KEY',
  defaultValue: 'aK3R5vXwJ9eeqQ1iYah9fzBAIHOeKNejoRdI8weXUYo=',
);

/// DEB and RPM packages have their own feed and key (FEAT-06): the Linux
/// anchor never vouches for Windows, macOS or Android installers.
const linuxUpdateFeedUrl = String.fromEnvironment(
  'CC_LINUX_UPDATE_FEED',
  defaultValue: 'https://updates.consolecrypt.dev/linux-stable.json',
);
const linuxUpdateVerificationKey = String.fromEnvironment(
  'CC_LINUX_UPDATE_PUBLIC_KEY',
  defaultValue: 'Fn0pzhn51t/i8e8VYt7/wJkkXa7n2PkYtILgnxsuVlc=',
);

/// Runs a program and waits for its exit code (the Linux package manager).
typedef PackageInstaller = Future<int> Function(String executable, List<String> arguments);

Future<int> _runAndWait(String executable, List<String> arguments) async =>
    (await Process.run(executable, arguments)).exitCode;

bool _exists(String path) => File(path).existsSync();

/// How this Linux copy was installed: the `consolecrypt` DEB or RPM in
/// /opt/consolecrypt, or neither (a build tree or a copied folder, which
/// keep the download page).
String linuxPackagePlatform() {
  if (Abi.current() != Abi.linuxX64 || Platform.resolvedExecutable != '/opt/consolecrypt/consolecrypt') {
    return 'unsupported';
  }
  if (_exists('/var/lib/dpkg/info/consolecrypt.list')) return 'linux-x64-deb';
  try {
    if (_exists('/usr/bin/rpm') && Process.runSync('/usr/bin/rpm', ['-q', '--quiet', 'consolecrypt']).exitCode == 0) {
      return 'linux-x64-rpm';
    }
  } on Object {
    // No usable RPM database: not an RPM installation.
  }
  return 'unsupported';
}

/// The package manager command `pkexec` runs for a verified [path].
List<String> linuxInstallCommand(String platform, String path, {bool Function(String) exists = _exists}) =>
    switch (platform) {
      'linux-x64-deb' when exists('/usr/bin/apt-get') => ['/usr/bin/apt-get', 'install', '--yes', path],
      'linux-x64-deb' => ['/usr/bin/dpkg', '--install', path],
      'linux-x64-rpm' when exists('/usr/bin/dnf') => ['/usr/bin/dnf', 'install', '--assumeyes', path],
      // The package is verified by the signed feed, not by an RPM signature.
      'linux-x64-rpm' when exists('/usr/bin/zypper') => [
        '/usr/bin/zypper',
        '--non-interactive',
        'install',
        '--allow-unsigned-rpm',
        path,
      ],
      'linux-x64-rpm' => ['/usr/bin/rpm', '--upgrade', path],
      _ => throw const UpdateException('platform'),
    };

class UpdateException implements Exception {
  const UpdateException(this.code, [this.detail]);
  final String code;

  /// Non-sensitive diagnostic, e.g. the OS error number of a failed launch.
  final String? detail;
  @override
  String toString() => 'UpdateException($code${detail == null ? '' : ', $detail'})';
}

/// The newest authenticated `issuedAt` this device has accepted. A replayed
/// older (still unexpired) feed must not hide a newer release for the rest of
/// its 100-day validity window.
abstract interface class UpdateFeedHighWater {
  Future<DateTime?> read();
  Future<void> write(DateTime issuedAt);
}

/// Starts a program without waiting for it (the Windows installer).
typedef InstallerLauncher = Future<void> Function(String executable, List<String> arguments);

Future<void> _startDetached(String executable, List<String> arguments) =>
    Process.start(executable, arguments, mode: ProcessStartMode.detached);

/// `\\?\C:\x` → `C:\x` and `\\?\UNC\host\share` → `\\host\share`. Verbatim
/// paths break programs that resolve themselves (Inno Setup) and must not be shown.
String withoutVerbatimPrefix(String path) {
  if (path.startsWith(r'\\?\UNC\')) return '\\\\${path.substring(8)}';
  if (RegExp(r'^\\\\\?\\[A-Za-z]:\\').hasMatch(path)) return path.substring(4);
  return path;
}

/// Inno Setup leaves its uninstaller (`unins000.exe`) next to the installed
/// app. A copy unpacked from the portable ZIP has none: the installer would
/// update a different folder, so that copy cannot update itself.
bool windowsInstalledByInstaller(Iterable<String> siblingNames) =>
    siblingNames.any((name) => RegExp(r'^unins\d{3}\.exe$', caseSensitive: false).hasMatch(name));

/// Inno Setup arguments for an in-place update. `/WAITPID` and `/RELAUNCH`
/// are read by ConsoleCrypt.iss: setup waits until this process has exited
/// (no files in use, no Restart Manager prompt) and starts the updated app.
List<String> windowsInstallerArguments(int processId) => [
  '/SP-',
  '/SILENT',
  '/NORESTART',
  '/CLOSEAPPLICATIONS',
  '/RELAUNCH=1',
  '/WAITPID=$processId',
];

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
        'linux-x64-deb' => 'linux-x64.deb',
        'linux-x64-rpm' => 'linux-x64.rpm',
        _ => throw const UpdateException('platform'),
      }}';
}

List<int> _version(String value) {
  if (!RegExp(r'^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$').hasMatch(value)) {
    throw const UpdateException('version');
  }
  return value.split('.').map(int.parse).toList();
}

/// Whether `candidate+candidateBuild` is newer than the running build. The
/// same version with a higher build (a rebuilt release) is an update too.
bool newerVersion(String candidate, String current, {int candidateBuild = 0, int currentBuild = 0}) {
  final a = _version(candidate);
  final b = _version(current);
  for (var i = 0; i < 3; i++) {
    if (a[i] != b[i]) return a[i] > b[i];
  }
  return candidateBuild > currentBuild;
}

bool safeUpdateUrl(Uri url) {
  if (url.scheme != 'https' || url.userInfo.isNotEmpty || url.port != 443 || url.hasFragment) return false;
  // Keep the legacy hosts during migration: already installed clients still
  // retrieve 0.3.2 from their original signed feed without foreign redirects.
  if (const {'git.evsikov.net', 'updates.consolecrypt.evsikov.net', 'updates.consolecrypt.dev'}.contains(url.host)) {
    return true;
  }
  if (url.host == 'github.com') {
    return RegExp(r'^/evsikovas/consolecrypt-client/releases/download/v\d+\.\d+\.\d+/ConsoleCrypt-[^/]+$')
            .hasMatch(url.path) &&
        !url.hasQuery;
  }
  // GitHub redirects release downloads to this exact CDN host. Installer
  // length and SHA-256 remain authenticated by the existing Ed25519 key.
  return url.host == 'release-assets.githubusercontent.com' &&
      RegExp(r'^/github-production-release-asset/\d+/[^/]+$').hasMatch(url.path);
}

/// Authenticates the exact signed bytes BEFORE reading any installer URL.
Future<UpdateRelease?> verifyUpdateFeed(
  List<int> envelope, {
  required String platform,
  String currentVersion = kAppVersion,
  int currentBuild = kAppBuildNumber,
  String publicKey = updateVerificationKey,
  DateTime? now,
  DateTime? notBefore,
  void Function(DateTime issuedAt)? onAuthenticated,
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
    if (notBefore != null && issued.isBefore(notBefore)) throw const UpdateException('manifest_rollback');
    onAuthenticated?.call(issued);
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
    if (!newerVersion(version, currentVersion, candidateBuild: build, currentBuild: currentBuild)) return null;
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

/// [installedRestart]: the Linux package manager replaced the app; it runs
/// the new version after a restart.
enum UpdateInstallResult { opened, exitWindows, androidPermission, cancelled, installedRestart }

class UpdateService {
  UpdateService({
    HttpClient Function()? clientFactory,
    this.publicKey = updateVerificationKey,
    this.linuxPublicKey = linuxUpdateVerificationKey,
    MethodChannel? macosChannel,
    this._feedHighWater,
    InstallerLauncher? launcher,
    this._platform,
    Future<bool> Function()? windowsInstalled,
    int? processId,
    PackageInstaller? packageInstaller,
    bool Function(String path)? fileExists,
  }) : _clientFactory = clientFactory ?? HttpClient.new,
       _packageInstaller = packageInstaller ?? _runAndWait,
       _fileExists = fileExists ?? _exists,
       _macos = macosChannel ?? const MethodChannel('consolecrypt/macos_updates'),
       _launch = launcher ?? _startDetached,
       _windowsInstalled = windowsInstalled ?? _probeWindowsInstall,
       _processId = processId ?? pid;
  final HttpClient Function() _clientFactory;
  final MethodChannel _macos;
  final String publicKey, linuxPublicKey;
  final PackageInstaller _packageInstaller;
  final bool Function(String path) _fileExists;
  final UpdateFeedHighWater? _feedHighWater;
  final InstallerLauncher _launch;
  final String? _platform;
  final Future<bool> Function() _windowsInstalled;
  final int _processId;
  static const _android = MethodChannel('consolecrypt/updates');

  /// The verified installer stays open (and locked) from the last hash check
  /// until this process exits, so its bytes cannot change before Setup reads them.
  RandomAccessFile? _launchedInstaller;

  /// Lets tests remove the installer's folder (Windows refuses while it is open).
  @visibleForTesting
  Future<void> releaseInstaller() async {
    await _launchedInstaller?.close();
    _launchedInstaller = null;
  }

  String get platform =>
      _platform ??
      (Platform.isWindows
          ? 'windows-x64'
          : Platform.isMacOS
          ? 'macos-universal'
          : Platform.isAndroid
          ? 'android-arm64'
          : Platform.isLinux
          ? _linux
          : 'unsupported');
  late final String _linux = linuxPackagePlatform();
  bool get _isLinux => platform.startsWith('linux-');

  static Future<bool> _probeWindowsInstall() async {
    try {
      final folder = File(withoutVerbatimPrefix(Platform.resolvedExecutable)).parent;
      final names = await folder
          .list(followLinks: false)
          .map((entry) => entry.uri.pathSegments.lastWhere((s) => s.isNotEmpty))
          .toList();
      return windowsInstalledByInstaller(names);
    } on Object {
      // Unknown: offering the download page is safe, a misdirected installer is not.
      return false;
    }
  }

  /// Whether the downloaded installer can update this copy in place. A
  /// portable Windows copy is sent to the download page instead.
  Future<bool> canInstallInPlace() async => platform != 'windows-x64' || await _windowsInstalled();

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
      if (response.statusCode != HttpStatus.ok) throw UpdateException('http', '${response.statusCode}');
      return response;
    }
    throw const UpdateException('redirect');
  }

  Future<UpdateRelease?> check() async {
    if (platform == 'unsupported') throw const UpdateException('platform');
    final client = _client();
    try {
      final response = await _get(client, Uri.parse(_isLinux ? linuxUpdateFeedUrl : updateFeedUrl));
      final bytes = <int>[];
      await for (final chunk in response.timeout(const Duration(seconds: 30))) {
        bytes.addAll(chunk);
        if (bytes.length > 65536) throw const UpdateException('manifest_size');
      }
      final now = DateTime.now().toUtc();
      var floor = await _feedHighWater?.read();
      // A corrupt future value must not block every later feed.
      if (floor != null && floor.isAfter(now.add(const Duration(minutes: 10)))) floor = null;
      DateTime? issued;
      final release = await verifyUpdateFeed(
        bytes,
        platform: platform,
        publicKey: _isLinux ? linuxPublicKey : publicKey,
        now: now,
        notBefore: floor,
        onAuthenticated: (value) => issued = value,
      );
      final accepted = issued;
      if (accepted != null && (floor == null || accepted.isAfter(floor))) await _feedHighWater?.write(accepted);
      return release;
    } finally {
      client.close(force: true);
    }
  }

  /// Removes installers left by earlier attempts (they are kept for the
  /// running Setup until this app exits).
  static Future<void> _removeStale(Directory root, String prefix) async {
    try {
      final cutoff = DateTime.now().subtract(const Duration(hours: 12));
      await for (final entry in root.list(followLinks: false)) {
        final name = entry.uri.pathSegments.lastWhere((s) => s.isNotEmpty);
        if (entry is! Directory || !name.startsWith(prefix)) continue;
        if (entry.statSync().modified.isBefore(cutoff)) await entry.delete(recursive: true);
      }
    } on Object {
      // Best effort only.
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
        await _removeStale(Directory.systemTemp, 'consolecrypt-update-');
        try {
          folder = await Directory.systemTemp.createTemp('consolecrypt-update-');
        } on FileSystemException catch (error) {
          throw UpdateException('storage', '${error.osError?.errorCode ?? ''}');
        }
      }
      // A fresh random folder and an exclusively created file: nothing else
      // chose this path, and the bytes are verified again right before launch.
      final file = File('${folder.path}${Platform.pathSeparator}${release.fileName}');
      try {
        await file.create(exclusive: true);
        output = await file.open(mode: FileMode.writeOnly);
      } on FileSystemException catch (error) {
        throw UpdateException('storage', '${error.osError?.errorCode ?? ''}');
      }
      final response = await _get(client, release.url);
      if (response.contentLength != -1 && response.contentLength != release.bytes) {
        throw const UpdateException('size');
      }
      var received = 0;
      progress(0);
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

  /// Hashes through [handle] itself, so the checked bytes are the open file's.
  static Future<void> _verifyOpen(RandomAccessFile handle, UpdateRelease release) async {
    if (await handle.length() != release.bytes) throw const UpdateException('checksum');
    await handle.setPosition(0);
    final sink = _DigestSink();
    final input = digest.sha256.startChunkedConversion(sink);
    var remaining = release.bytes;
    while (remaining > 0) {
      final chunk = await handle.read(remaining < 1 << 20 ? remaining : 1 << 20);
      if (chunk.isEmpty) throw const UpdateException('checksum');
      input.add(chunk);
      remaining -= chunk.length;
    }
    input.close();
    if (sink.value.toString() != release.sha256) throw const UpdateException('checksum');
  }

  /// Windows: open the installer (Dart's handle denies delete/rename while
  /// open), take a shared lock (no process can write the bytes), hash those
  /// bytes and only then start Setup. The handle stays open until exit.
  Future<UpdateInstallResult> _installWindows(File file, UpdateRelease release) async {
    final RandomAccessFile handle;
    try {
      handle = await file.open();
    } on FileSystemException {
      throw const UpdateException('checksum');
    }
    try {
      try {
        await handle.lock(FileLock.shared, 0, release.bytes);
      } on FileSystemException {
        throw const UpdateException('checksum');
      }
      await _verifyOpen(handle, release);
      try {
        await _launch(withoutVerbatimPrefix(file.absolute.path), windowsInstallerArguments(_processId));
      } on ProcessException catch (error) {
        throw UpdateException('launch', '${error.errorCode}');
      }
      await _launchedInstaller?.close();
      _launchedInstaller = handle;
      return UpdateInstallResult.exitWindows;
    } on Object {
      await handle.close();
      rethrow;
    }
  }

  /// Linux: the system package manager installs the verified package through
  /// `pkexec` (the polkit password dialog names the command). Profiles, the
  /// vault and the keyring stay; the app runs the new version after a restart.
  Future<UpdateInstallResult> _installLinux(File file, UpdateRelease release) async {
    const pkexec = '/usr/bin/pkexec';
    if (!_fileExists(pkexec)) throw const UpdateException('pkexec');
    final command = linuxInstallCommand(release.platform, file.absolute.path, exists: _fileExists);
    final int code;
    try {
      code = await _packageInstaller(pkexec, command);
    } on ProcessException catch (error) {
      throw UpdateException('launch', '${error.errorCode}');
    }
    // pkexec: 126 = the password dialog was dismissed, 127 = not authorized.
    if (code == 126 || code == 127) return UpdateInstallResult.cancelled;
    if (code != 0) throw UpdateException('installer', '$code');
    return UpdateInstallResult.installedRestart;
  }

  Future<UpdateInstallResult> install(
    File file,
    UpdateRelease release, {
    String? macosSaveTitle,
    String? macosSavePrompt,
  }) async {
    if (release.platform != platform) throw const UpdateException('platform');
    if (platform == 'windows-x64') {
      if (!await canInstallInPlace()) throw const UpdateException('portable');
      return _installWindows(file, release);
    }
    await verifyInstaller(file, release);
    if (_isLinux) return _installLinux(file, release);
    if (Platform.isMacOS) {
      // Opening the sandbox cache directly propagates quarantine's
      // no-user-consent bit to the app copied out of the DMG. The native
      // bridge creates fresh verified bytes at an NSSavePanel-approved URL.
      final result = await _macos.invokeMethod<String>('saveAndOpen', {
        'path': file.path,
        'fileName': release.fileName,
        'bytes': release.bytes,
        'sha256': release.sha256,
        'title': macosSaveTitle ?? 'ConsoleCrypt',
        'prompt': macosSavePrompt,
      });
      if (result == 'cancelled') return UpdateInstallResult.cancelled;
      if (result != 'opened') throw UpdateException(result == 'destination_exists' ? result! : 'installer');
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

final class _DigestSink implements Sink<digest.Digest> {
  late digest.Digest value;
  @override
  void add(digest.Digest data) => value = data;
  @override
  void close() {}
}
