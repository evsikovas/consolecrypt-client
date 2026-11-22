import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart' show Json, localSettingsFromJson, localSettingsToJson;
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/settings_service.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:path/path.dart' as path;

/// Standalone windows can start before a vault is opened. Keep only their
/// display preferences here, separately from profiles and main-window state.
final class TerminalOnlySettings implements SettingsService {
  TerminalOnlySettings._(this._file, LocalSettings initial) : _state = ValueStreamController(initial);

  static const defaults = LocalSettings(
    terminalTabPlacement: TerminalTabPlacement.left,
    terminalTabColumnCollapsed: true,
    terminalTabSubtitle: TerminalTabSubtitle.off,
  );
  static const _keys = {
    'theme_mode',
    'glass_mode',
    'ui_font_scale',
    'ui_font_scale_version',
    'ui_accent_color',
    'ui_background_color',
    'terminal_font_size',
    'terminal_font_family',
    'terminal_color_scheme',
    'custom_terminal_colors',
    'terminal_tab_placement',
    'terminal_tab_subtitle',
    'terminal_tab_column_width',
    'terminal_tab_column_collapsed',
    'terminal_scrollback',
  };

  final File _file;
  final ValueStreamController<LocalSettings> _state;
  Future<void> _writes = Future.value();

  static File _defaultFile() {
    final env = Platform.environment;
    final home = env['HOME'];
    final root =
        env['CONSOLECRYPT_DATA_DIR'] ??
        (Platform.isWindows
            ? env['LOCALAPPDATA']
            : Platform.isMacOS && home != null
            ? path.join(home, 'Library', 'Application Support')
            : env['XDG_CONFIG_HOME'] ?? (home == null ? null : path.join(home, '.config')));
    if (root == null || root.isEmpty) throw const FileSystemException('No terminal settings directory');
    return File(path.join(root, 'ConsoleCrypt', 'terminal-only.json'));
  }

  static Json _display(LocalSettings settings) =>
      localSettingsToJson(settings)..removeWhere((key, _) => !_keys.contains(key));

  static Future<Json> _read(File file) async {
    try {
      final data = jsonDecode(await file.readAsString());
      if (data is Map<String, dynamic>) return Map.of(data)..removeWhere((key, _) => !_keys.contains(key));
    } on PathNotFoundException {
      // A first launch has no settings file.
    } on FormatException {
      // Start with usable defaults if an older preferences file is truncated.
    }
    return {};
  }

  static Future<TerminalOnlySettings> open({File? file}) async {
    final target = file ?? _defaultFile();
    final values = {..._display(defaults), ...await _read(target)};
    return TerminalOnlySettings._(target, localSettingsFromJson(values));
  }

  @override
  Stream<LocalSettings> watchLocal() => _state.stream;
  @override
  LocalSettings get currentLocal => _state.value;

  // Two separately launched windows can edit different controls at the same
  // time. Merge only the changed fields under a process lock before replacing
  // the file. Slider writes from this window are applied in gesture order.
  @override
  Future<void> updateLocal(LocalSettings settings) {
    final before = _display(currentLocal);
    final changes = _display(settings)
      ..removeWhere((key, value) {
        final previous = before[key];
        return value is Map && previous is Map ? jsonEncode(value) == jsonEncode(previous) : value == previous;
      });
    if (changes.isEmpty) return _writes;
    _state.value = settings;
    final write = _writes.then((_) async {
      await _file.parent.create(recursive: true);
      final lock = await File('${_file.path}.lock').open(mode: FileMode.append);
      try {
        await lock.lock(FileLock.blockingExclusive);
        final data = {..._display(defaults), ...await _read(_file), ...changes};
        final temporary = File('${_file.path}.$pid.tmp');
        await temporary.writeAsString(jsonEncode(data), flush: true);
        await temporary.rename(_file.path);
      } finally {
        await lock.close();
      }
    });
    _writes = write.catchError((Object _) {});
    return write;
  }

  Future<void> flush() => _writes;

  @override
  Stream<VaultSettings?> watchVault() => Stream.value(null);
  @override
  Future<void> updateVault(VaultSettings settings) => throw UnsupportedError('No vault in Terminal Only');
}
