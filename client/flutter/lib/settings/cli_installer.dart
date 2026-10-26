import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The command the desktop packages carry (`consolecrypt` itself starts the
/// app on Linux).
const cliCommand = 'consolecrypt-cli';

/// Where the bundled command is and whether the user's shell finds it.
@immutable
final class CliCommandState {
  const CliCommandState({required this.bundled, required this.installed, this.location, this.onPath = true});

  /// Path of the binary inside this installation; null when this build has
  /// none (development builds, phones).
  final String? bundled;
  final bool installed;

  /// The link (macOS, Linux) or the PATH folder (Windows).
  final String? location;

  /// Linux: whether `~/.local/bin` is on PATH (new shells find the command).
  final bool onPath;
}

final class CliInstallException implements Exception {
  const CliInstallException(this.code, [this.detail]);

  /// `cancelled` (password dialog closed), `conflict` (a foreign file is in
  /// the way), `failed`.
  final String code;
  final String? detail;
  @override
  String toString() => 'CliInstallException($code${detail == null ? '' : ', $detail'})';
}

typedef CliProcessRunner = Future<ProcessResult> Function(
  String executable,
  List<String> arguments, {
  Map<String, String>? environment,
});

Future<ProcessResult> _run(String executable, List<String> arguments, {Map<String, String>? environment}) =>
    Process.run(executable, arguments, environment: environment);

/// Puts the bundled `consolecrypt-cli` on the user's command line:
/// * macOS: a link in /usr/local/bin (asks for the administrator password
///   when that folder is not writable);
/// * Linux: a link in ~/.local/bin;
/// * Windows: the installation's `bin` folder in the user's PATH.
/// Removing only touches what points at this installation.
class CliInstaller {
  CliInstaller({
    String? executable,
    String? home,
    String? systemBin,
    Map<String, String>? environment,
    CliProcessRunner? run,
    TargetPlatform? platform,
  }) : _executable = executable ?? Platform.resolvedExecutable,
       _home = home ?? Platform.environment['HOME'] ?? '',
       _systemBin = systemBin ?? '/usr/local/bin',
       _environment = environment ?? Platform.environment,
       _runProcess = run ?? _run,
       _platform = platform ?? defaultTargetPlatform;

  final String _executable, _home, _systemBin;
  final Map<String, String> _environment;
  final CliProcessRunner _runProcess;
  final TargetPlatform _platform;

  bool get supported => const {TargetPlatform.macOS, TargetPlatform.windows, TargetPlatform.linux}.contains(_platform);

  String get _appFolder => File(_executable).parent.path;

  String? get _bundled {
    final path = switch (_platform) {
      TargetPlatform.macOS => '${File(_executable).parent.parent.path}/Helpers/$cliCommand',
      TargetPlatform.windows => '$_appFolder\\bin\\$cliCommand.exe',
      TargetPlatform.linux => '$_appFolder/$cliCommand',
      _ => null,
    };
    return path != null && File(path).existsSync() ? path : null;
  }

  String get _windowsFolder => '$_appFolder\\bin';
  String get _link => _platform == TargetPlatform.macOS ? '$_systemBin/$cliCommand' : '$_home/.local/bin/$cliCommand';

  Future<CliCommandState> state() async {
    final bundled = _bundled;
    if (!supported || bundled == null) return const CliCommandState(bundled: null, installed: false);
    if (_platform == TargetPlatform.windows) {
      final folders = await _userPath();
      return CliCommandState(
        bundled: bundled,
        installed: folders.any((f) => _sameFolder(f, _windowsFolder)),
        location: _windowsFolder,
      );
    }
    final link = Link(_link);
    final installed = link.existsSync() && _linkTarget(link) == bundled;
    final linkFolder = File(_link).parent.path;
    final onPath = (_environment['PATH'] ?? '').split(':').any((p) => p == linkFolder || p == '$linkFolder/');
    return CliCommandState(bundled: bundled, installed: installed, location: _link, onPath: onPath);
  }

  Future<void> install() async {
    final bundled = _bundled ?? (throw const CliInstallException('failed', 'not_bundled'));
    switch (_platform) {
      case TargetPlatform.windows:
        await _windowsPath(add: true);
      case TargetPlatform.macOS:
        _refuseForeign();
        try {
          _replaceLink(bundled);
        } on FileSystemException {
          // /usr/local/bin is usually root's: the system asks for the password.
          await _asAdministrator('/bin/mkdir -p ', ' && /bin/ln -sfn ', [_systemBin, bundled, _link]);
        }
      default:
        _refuseForeign();
        try {
          Directory(File(_link).parent.path).createSync(recursive: true);
          _replaceLink(bundled);
        } on FileSystemException catch (e) {
          throw CliInstallException('failed', e.osError?.message);
        }
    }
  }

  Future<void> uninstall() async {
    if (_platform == TargetPlatform.windows) {
      await _windowsPath(add: false);
      return;
    }
    final link = Link(_link);
    if (!link.existsSync() || !_ours(_linkTarget(link))) return;
    try {
      link.deleteSync();
    } on FileSystemException {
      if (_platform != TargetPlatform.macOS) rethrow;
      await _asAdministrator('/bin/rm -f ', '', [_link]);
    }
  }

  // ── macOS and Linux ─────────────────────────────────────────────────────

  String? _linkTarget(Link link) {
    try {
      return link.targetSync();
    } on FileSystemException {
      return null;
    }
  }

  /// A link into a ConsoleCrypt installation (this or an earlier location).
  bool _ours(String? target) =>
      target != null &&
      (target == _bundled ||
          target.endsWith('ConsoleCrypt.app/Contents/Helpers/$cliCommand') ||
          target == '/opt/consolecrypt/$cliCommand');

  /// Never overwrite a file or someone else's link of the same name.
  void _refuseForeign() {
    final type = FileSystemEntity.typeSync(_link, followLinks: false);
    if (type == FileSystemEntityType.notFound) return;
    if (type != FileSystemEntityType.link || !_ours(_linkTarget(Link(_link)))) {
      throw CliInstallException('conflict', _link);
    }
  }

  void _replaceLink(String target) {
    final link = Link(_link);
    if (link.existsSync()) link.deleteSync();
    link.createSync(target);
  }

  /// Runs `/bin/sh -c` as root through the standard macOS password dialog;
  /// paths reach the shell only via AppleScript's `quoted form of`.
  Future<void> _asAdministrator(String first, String second, List<String> paths) async {
    final script = switch (paths.length) {
      3 =>
        'do shell script "$first" & quoted form of item 1 of argv & "$second" & quoted form of item 2 of argv ' // l10n-ignore: AppleScript / PowerShell source
            '& " " & quoted form of item 3 of argv with administrator privileges', // l10n-ignore: AppleScript / PowerShell source
      _ => 'do shell script "$first" & quoted form of item 1 of argv with administrator privileges', // l10n-ignore: AppleScript / PowerShell source
    };
    final result = await _runProcess('/usr/bin/osascript', [
      '-e',
      'on run argv', // l10n-ignore: AppleScript / PowerShell source
      '-e',
      script,
      '-e',
      'end run', // l10n-ignore: AppleScript / PowerShell source
      ...paths,
    ]);
    if (result.exitCode == 0) return;
    final error = '${result.stderr}';
    if (error.contains('-128')) throw const CliInstallException('cancelled');
    throw CliInstallException('failed', error.trim());
  }

  // ── Windows ─────────────────────────────────────────────────────────────

  static bool _sameFolder(String a, String b) {
    String clean(String s) => s.trim().replaceAll('/', '\\').replaceAll(RegExp(r'\\+$'), '').toLowerCase();
    return clean(a) == clean(b);
  }

  // The folder travels in an environment variable, never inside the script.
  static const _readPath = r"(Get-Item 'HKCU:\Environment').GetValue('Path', '', 'DoNotExpandEnvironmentNames')"; // l10n-ignore: AppleScript / PowerShell source

  Future<List<String>> _userPath() async {
    final result = await _runProcess('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', _readPath]);
    if (result.exitCode != 0) return const [];
    return '${result.stdout}'.trim().split(';').where((p) => p.trim().isNotEmpty).toList();
  }

  Future<void> _windowsPath({required bool add}) async {
    final script =
        '\$d = \$env:CC_CLI_DIR; \$p = $_readPath; '
        r"$parts = @($p -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ine $d.TrimEnd('\')) }); "
        '${add ? r'$parts += $d; ' : ''}'
        // SetEnvironmentVariable notifies running programs (new terminals see it).
        r"[Environment]::SetEnvironmentVariable('Path', ($parts -join ';'), 'User')";
    final result = await _runProcess(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command', script],
      environment: {'CC_CLI_DIR': _windowsFolder},
    );
    if (result.exitCode != 0) throw CliInstallException('failed', '${result.stderr}'.trim());
  }
}

final cliInstallerProvider = Provider<CliInstaller>((ref) => CliInstaller());
final cliCommandStateProvider = FutureProvider.autoDispose<CliCommandState>(
  (ref) => ref.watch(cliInstallerProvider).state(),
);
