import 'dart:typed_data';

/// Kind of a remote directory entry as reported by `lstat`
/// (`sftp_core::EntryKind`; wire names = serde snake_case).
enum RemoteEntryKind {
  file('file'),
  directory('dir'),
  symlink('symlink'),
  other('other');

  const RemoteEntryKind(this.wireName);

  final String wireName;

  static RemoteEntryKind fromWire(String name) => values.firstWhere((k) => k.wireName == name, orElse: () => other);
}

/// A remote entry with full metadata: `sftp_core::RemoteEntry` plus the
/// symlink target (`SftpBrowserService.listDirectory` / `stat`).
final class RemoteFileInfo {
  const RemoteFileInfo({
    required this.name,
    required this.path,
    required this.kind,
    this.size = 0,
    this.permissions,
    this.uid,
    this.gid,
    this.owner,
    this.group,
    this.modifiedAt,
    this.linkTarget,
    this.linkTargetKind,
  });

  final String name;

  /// Absolute POSIX path (as listed, symlinks not resolved).
  final String path;

  /// `lstat` kind: a symlink is [RemoteEntryKind.symlink] whatever it points to.
  final RemoteEntryKind kind;
  final int size;

  /// Permission bits (`0o7777` mask incl. setuid/setgid/sticky), if reported.
  final int? permissions;
  final int? uid;
  final int? gid;

  /// User / group names (`longname` of SFTP v3 or name lookup); `null` → show uid/gid.
  final String? owner;
  final String? group;
  final DateTime? modifiedAt;

  /// Symlinks only: the link text (`readlink`) and the kind of the final
  /// target (`stat`; `null` = dangling link).
  final String? linkTarget;
  final RemoteEntryKind? linkTargetKind;

  bool get isSymlink => kind == RemoteEntryKind.symlink;

  /// Directories and symlinks to directories open as folders.
  bool get isDirectory =>
      kind == RemoteEntryKind.directory || (isSymlink && linkTargetKind == RemoteEntryKind.directory);

  /// Dot-files (shown dimmed, hidden unless "Show hidden files").
  bool get isHidden => name.startsWith('.');

  bool get isExecutable => !isDirectory && ((permissions ?? 0) & 0x49) != 0; // any x bit (0o111)

  /// Lower-case extension without the dot (`tar.gz` → `gz`), empty if none.
  String get extension {
    final dot = name.lastIndexOf('.');
    return dot <= 0 || dot == name.length - 1 ? '' : name.substring(dot + 1).toLowerCase();
  }

  /// `ls -l` style: `drwxr-xr-x`, `lrwxrwxrwx`, `-rwsr-x---`.
  String get modeString => formatModeString(kind, permissions);

  RemoteFileInfo copyWith({String? name, String? path, int? size, int? permissions, DateTime? modifiedAt}) =>
      RemoteFileInfo(
        name: name ?? this.name,
        path: path ?? this.path,
        kind: kind,
        size: size ?? this.size,
        permissions: permissions ?? this.permissions,
        uid: uid,
        gid: gid,
        owner: owner,
        group: group,
        modifiedAt: modifiedAt ?? this.modifiedAt,
        linkTarget: linkTarget,
        linkTargetKind: linkTargetKind,
      );

  @override
  bool operator ==(Object other) =>
      other is RemoteFileInfo &&
      other.path == path &&
      other.kind == kind &&
      other.size == size &&
      other.permissions == permissions &&
      other.modifiedAt == modifiedAt &&
      other.owner == owner &&
      other.group == group &&
      other.linkTarget == linkTarget;

  @override
  int get hashCode => Object.hash(path, kind, size, permissions, modifiedAt, owner, group, linkTarget);
}

/// `drwxr-xr-x` for [kind] + [permissions] (`?` type for unknown kinds;
/// `----------` when the server reported no mode).
String formatModeString(RemoteEntryKind kind, int? permissions) {
  final type = switch (kind) {
    RemoteEntryKind.directory => 'd',
    RemoteEntryKind.symlink => 'l',
    RemoteEntryKind.file => '-',
    RemoteEntryKind.other => '?',
  };
  final p = permissions ?? 0;
  final buffer = StringBuffer(type);
  const specials = [0x800, 0x400, 0x200]; // setuid, setgid, sticky
  for (var i = 0; i < 3; i++) {
    final shift = 6 - i * 3;
    final r = p & (4 << shift) != 0;
    final w = p & (2 << shift) != 0;
    final x = p & (1 << shift) != 0;
    final special = p & specials[i] != 0;
    buffer
      ..write(r ? 'r' : '-')
      ..write(w ? 'w' : '-');
    if (special) {
      final lower = i == 2 ? 't' : 's';
      buffer.write(x ? lower : lower.toUpperCase());
    } else {
      buffer.write(x ? 'x' : '-');
    }
  }
  return buffer.toString();
}

/// Inverse of [formatModeString] for the permission part (the first
/// character is ignored). Returns `null` for malformed input.
int? parseModeString(String mode) {
  if (mode.length != 10) return null;
  var result = 0;
  const specials = [0x800, 0x400, 0x200];
  for (var i = 0; i < 3; i++) {
    final shift = 6 - i * 3;
    final chunk = mode.substring(1 + i * 3, 4 + i * 3);
    if (chunk[0] == 'r') result |= 4 << shift;
    if (chunk[1] == 'w') result |= 2 << shift;
    switch (chunk[2]) {
      case 'x':
        result |= 1 << shift;
      case 's' || 't':
        result |= (1 << shift) | specials[i];
      case 'S' || 'T':
        result |= specials[i];
      case '-':
        break;
      default:
        return null;
    }
  }
  return result;
}

/// `0755` / `4755` — always at least four octal digits.
String formatOctalMode(int mode) => (mode & 0xFFF).toRadixString(8).padLeft(4, '0');

/// Parses `755`, `0755`, `4755`; `null` if not 3–4 octal digits.
int? parseOctalMode(String text) {
  final t = text.trim();
  if (!RegExp(r'^[0-7]{3,4}$').hasMatch(t)) return null;
  return int.parse(t, radix: 8);
}

/// A bounded, in-memory prefix of a remote file for Quick Look
/// (`SftpBrowserService.readPreview`). Never written to disk.
final class SftpPreview {
  SftpPreview({required this.path, required this.bytes, required this.totalSize});

  final String path;
  final Uint8List bytes;

  /// Size of the whole remote file.
  final int totalSize;

  /// Only the first [bytes] of the file were read.
  bool get truncated => bytes.length < totalSize;
}

/// Columns of the remote file list (wire names for the stored preferences).
enum SftpListColumn {
  name('name'),
  size('size'),
  kind('kind'),
  modified('modified'),
  permissions('permissions'),
  owner('owner'),
  group('group');

  const SftpListColumn(this.wireName);

  final String wireName;

  static SftpListColumn? fromWire(String name) => values.where((c) => c.wireName == name).firstOrNull;
}

/// Device-local look & feel of the SFTP browser (never synced; stored by
/// the core as an opaque preferences record, `SftpBrowserService`).
final class SftpBrowserPreferences {
  const SftpBrowserPreferences({
    this.showHidden = false,
    this.foldersFirst = true,
    this.showLocalPane = false,
    this.editHintAcknowledged = false,
    this.sortColumn = SftpListColumn.name,
    this.sortAscending = true,
    this.columnOrder = defaultColumnOrder,
    this.visibleColumns = defaultVisibleColumns,
    this.columnWidths = const {},
  });

  static const defaultColumnOrder = SftpListColumn.values;
  static const defaultVisibleColumns = {
    SftpListColumn.name,
    SftpListColumn.size,
    SftpListColumn.kind,
    SftpListColumn.modified,
    SftpListColumn.permissions,
  };

  /// Default widths of the fixed columns (Name fills the rest).
  static const defaultWidths = {
    SftpListColumn.size: 84.0,
    SftpListColumn.kind: 140.0,
    SftpListColumn.modified: 156.0,
    SftpListColumn.permissions: 104.0,
    SftpListColumn.owner: 96.0,
    SftpListColumn.group: 96.0,
  };

  static const minColumnWidth = 56.0;
  static const maxColumnWidth = 480.0;

  final bool showHidden;
  final bool foldersFirst;
  final bool showLocalPane;

  /// The first-use hint of "edit in external app" was confirmed.
  final bool editHintAcknowledged;
  final SftpListColumn sortColumn;
  final bool sortAscending;

  /// Display order; Name is always first.
  final List<SftpListColumn> columnOrder;
  final Set<SftpListColumn> visibleColumns;

  /// User-resized widths (fixed columns only).
  final Map<SftpListColumn, double> columnWidths;

  double widthOf(SftpListColumn column) => columnWidths[column] ?? defaultWidths[column] ?? 120;

  SftpBrowserPreferences copyWith({
    bool? showHidden,
    bool? foldersFirst,
    bool? showLocalPane,
    bool? editHintAcknowledged,
    SftpListColumn? sortColumn,
    bool? sortAscending,
    List<SftpListColumn>? columnOrder,
    Set<SftpListColumn>? visibleColumns,
    Map<SftpListColumn, double>? columnWidths,
  }) => SftpBrowserPreferences(
    showHidden: showHidden ?? this.showHidden,
    foldersFirst: foldersFirst ?? this.foldersFirst,
    showLocalPane: showLocalPane ?? this.showLocalPane,
    editHintAcknowledged: editHintAcknowledged ?? this.editHintAcknowledged,
    sortColumn: sortColumn ?? this.sortColumn,
    sortAscending: sortAscending ?? this.sortAscending,
    columnOrder: columnOrder ?? this.columnOrder,
    visibleColumns: visibleColumns ?? this.visibleColumns,
    columnWidths: columnWidths ?? this.columnWidths,
  );

  /// JSON-compatible map (the core stores it verbatim).
  Map<String, Object> toJson() => {
    'show_hidden': showHidden,
    'folders_first': foldersFirst,
    'show_local_pane': showLocalPane,
    'edit_hint_acknowledged': editHintAcknowledged,
    'sort_column': sortColumn.wireName,
    'sort_ascending': sortAscending,
    'column_order': [for (final c in columnOrder) c.wireName],
    'visible_columns': [for (final c in visibleColumns) c.wireName],
    'column_widths': {for (final e in columnWidths.entries) e.key.wireName: e.value},
  };

  /// Tolerant parser: unknown / malformed fields fall back to defaults.
  static SftpBrowserPreferences fromJson(Map<String, Object?> json) {
    List<SftpListColumn> columns(Object? raw) =>
        raw is List ? [for (final v in raw) ?SftpListColumn.fromWire('$v')] : const <SftpListColumn>[];
    final order = columns(json['column_order']);
    final visible = columns(json['visible_columns']).toSet();
    final widths = <SftpListColumn, double>{};
    final rawWidths = json['column_widths'];
    if (rawWidths is Map) {
      for (final e in rawWidths.entries) {
        final column = SftpListColumn.fromWire('${e.key}');
        final value = e.value;
        if (column != null && value is num) widths[column] = value.toDouble();
      }
    }
    return SftpBrowserPreferences(
      showHidden: json['show_hidden'] == true,
      foldersFirst: json['folders_first'] != false,
      showLocalPane: json['show_local_pane'] == true,
      editHintAcknowledged: json['edit_hint_acknowledged'] == true,
      sortColumn: SftpListColumn.fromWire('${json['sort_column']}') ?? SftpListColumn.name,
      sortAscending: json['sort_ascending'] != false,
      columnOrder: order.length == SftpListColumn.values.length && order.first == SftpListColumn.name
          ? order
          : defaultColumnOrder,
      visibleColumns: visible.contains(SftpListColumn.name) ? visible : defaultVisibleColumns,
      columnWidths: widths,
    );
  }
}
