import 'package:consolecrypt/core/models/sftp_browser.dart';
import 'package:consolecrypt/sftp/file_kind.dart';

/// One visible row of the remote list: an entry at a tree depth (inline
/// disclosure), or the "loading…" placeholder under an expanding folder.
final class SftpRow {
  const SftpRow(this.entry, this.depth, {this.expanded = false, this.loading = false});

  final RemoteFileInfo entry;
  final int depth;
  final bool expanded;

  /// The folder's children are being fetched (expanded, not loaded yet).
  final bool loading;

  String get path => entry.path;
}

/// "a2" < "a10", case-insensitive; ties broken case-sensitively.
int compareNatural(String a, String b) {
  final x = a.toLowerCase();
  final y = b.toLowerCase();
  var i = 0;
  var j = 0;
  while (i < x.length && j < y.length) {
    final cx = x.codeUnitAt(i);
    final cy = y.codeUnitAt(j);
    final dx = cx >= 48 && cx <= 57;
    final dy = cy >= 48 && cy <= 57;
    if (dx && dy) {
      var ei = i;
      var ej = j;
      while (ei < x.length && x.codeUnitAt(ei) >= 48 && x.codeUnitAt(ei) <= 57) {
        ei++;
      }
      while (ej < y.length && y.codeUnitAt(ej) >= 48 && y.codeUnitAt(ej) <= 57) {
        ej++;
      }
      final nx = BigInt.parse(x.substring(i, ei));
      final ny = BigInt.parse(y.substring(j, ej));
      final c = nx.compareTo(ny);
      if (c != 0) return c;
      i = ei;
      j = ej;
    } else {
      if (cx != cy) return cx.compareTo(cy);
      i++;
      j++;
    }
  }
  final rest = (x.length - i).compareTo(y.length - j);
  return rest != 0 ? rest : a.compareTo(b);
}

/// Sort order of the list for [prefs] (column, direction, folders first).
int Function(RemoteFileInfo, RemoteFileInfo) sftpComparator(SftpBrowserPreferences prefs) {
  int primary(RemoteFileInfo a, RemoteFileInfo b) => switch (prefs.sortColumn) {
    SftpListColumn.name => compareNatural(a.name, b.name),
    SftpListColumn.size => (a.isDirectory ? -1 : a.size).compareTo(b.isDirectory ? -1 : b.size),
    SftpListColumn.kind => fileKindOf(a).sortKey.compareTo(fileKindOf(b).sortKey),
    SftpListColumn.modified => (a.modifiedAt?.millisecondsSinceEpoch ?? 0).compareTo(
      b.modifiedAt?.millisecondsSinceEpoch ?? 0,
    ),
    SftpListColumn.permissions => (a.permissions ?? -1).compareTo(b.permissions ?? -1),
    SftpListColumn.owner => (a.owner ?? '${a.uid ?? ''}').compareTo(b.owner ?? '${b.uid ?? ''}'),
    SftpListColumn.group => (a.group ?? '${a.gid ?? ''}').compareTo(b.group ?? '${b.gid ?? ''}'),
  };
  final direction = prefs.sortAscending ? 1 : -1;
  return (a, b) {
    if (prefs.foldersFirst && a.isDirectory != b.isDirectory) return a.isDirectory ? -1 : 1;
    final c = primary(a, b) * direction;
    return c != 0 ? c : compareNatural(a.name, b.name);
  };
}

/// Flattens the directory cache into visible rows: [root]'s entries sorted,
/// each expanded folder followed by its (lazily loaded) children. Hidden
/// files are dropped unless shown; a non-empty [filter] keeps entries whose
/// name contains it (case-insensitive) plus the folders leading to them.
List<SftpRow> buildSftpRows({
  required String root,
  required Map<String, List<RemoteFileInfo>> dirs,
  required Set<String> expanded,
  required Set<String> loading,
  required SftpBrowserPreferences prefs,
  String filter = '',
}) {
  final compare = sftpComparator(prefs);
  final query = filter.trim().toLowerCase();
  final rows = <SftpRow>[];

  bool matches(RemoteFileInfo e) => query.isEmpty || e.name.toLowerCase().contains(query);

  bool subtreeMatches(RemoteFileInfo e, int depth) {
    if (matches(e)) return true;
    if (!e.isDirectory || !expanded.contains(e.path) || depth > 32) return false;
    return (dirs[e.path] ?? const [])
        .where((c) => prefs.showHidden || !c.isHidden)
        .any((c) => subtreeMatches(c, depth + 1));
  }

  void add(String path, int depth, Set<String> ancestors) {
    final children = [
      for (final e in dirs[path] ?? const <RemoteFileInfo>[])
        if ((prefs.showHidden || !e.isHidden) && subtreeMatches(e, depth)) e,
    ]..sort(compare);
    for (final e in children) {
      final open = e.isDirectory && expanded.contains(e.path) && !ancestors.contains(e.path);
      final pending = open && !dirs.containsKey(e.path);
      rows.add(SftpRow(e, depth, expanded: open, loading: pending && loading.contains(e.path)));
      if (open && !pending) add(e.path, depth + 1, {...ancestors, e.path});
    }
  }

  add(root, 0, {root});
  return rows;
}

/// Summary of [entries] for the status bar.
final class SftpCounts {
  const SftpCounts({required this.folders, required this.files, required this.bytes});

  factory SftpCounts.of(Iterable<RemoteFileInfo> entries) {
    var folders = 0;
    var files = 0;
    var bytes = 0;
    for (final e in entries) {
      if (e.isDirectory) {
        folders++;
      } else {
        files++;
        if (!e.isSymlink) bytes += e.size;
      }
    }
    return SftpCounts(folders: folders, files: files, bytes: bytes);
  }

  final int folders;
  final int files;
  final int bytes;

  int get total => folders + files;
}

/// Columns that fit into [width] (Name keeps at least [minNameWidth];
/// optional columns are dropped in a fixed priority order).
List<SftpListColumn> fitColumns(SftpBrowserPreferences prefs, double width, {double minNameWidth = 200}) {
  final visible = [
    for (final c in prefs.columnOrder)
      if (prefs.visibleColumns.contains(c)) c,
  ];
  const dropOrder = [
    SftpListColumn.group,
    SftpListColumn.owner,
    SftpListColumn.permissions,
    SftpListColumn.kind,
    SftpListColumn.modified,
    SftpListColumn.size,
  ];
  double fixed(List<SftpListColumn> cols) =>
      cols.where((c) => c != SftpListColumn.name).fold(0, (sum, c) => sum + prefs.widthOf(c));
  for (final drop in dropOrder) {
    if (width - fixed(visible) >= minNameWidth) break;
    visible.remove(drop);
  }
  return visible;
}
