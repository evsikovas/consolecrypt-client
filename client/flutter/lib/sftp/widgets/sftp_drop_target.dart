import 'package:desktop_drop/desktop_drop.dart';
import 'package:material_ui/material_ui.dart';

/// Files dropped from Finder / Explorer (the only place that touches the
/// OS drag-and-drop plugin, so it can be swapped).
///
/// Drop-in only. Drag-out to Finder/Explorer needs file promises
/// (`NSFilePromiseProvider`, Windows virtual files) fed by a streaming
/// download API; the permissively licensed package that supports it on
/// both platforms (`super_drag_and_drop`, MIT) builds a Rust native library
/// through cargokit (precompiled binaries downloaded by default) — deferred,
/// see the SFTP browser report / TODO below.
// TODO(sftp-ui): drag-out to Finder/Explorer — needs virtual-file DnD + a streaming download in
// SftpBrowserService; next: evaluate super_drag_and_drop with source builds (no downloaded binaries).
class SftpDropTarget extends StatelessWidget {
  const SftpDropTarget({required this.child, required this.onDrop, super.key, this.onHover, this.onExit});

  final Widget child;

  /// Local paths of the dropped files/folders, pointer position in this widget.
  final void Function(List<String> paths, Offset localPosition) onDrop;
  final ValueChanged<Offset>? onHover;
  final VoidCallback? onExit;

  @override
  Widget build(BuildContext context) => DropTarget(
    onDragEntered: (d) => onHover?.call(d.localPosition),
    onDragUpdated: (d) => onHover?.call(d.localPosition),
    onDragExited: (_) => onExit?.call(),
    onDragDone: (d) {
      onExit?.call();
      final paths = [
        for (final f in d.files)
          if (f.path.isNotEmpty) f.path,
      ];
      if (paths.isNotEmpty) onDrop(paths, d.localPosition);
    },
    child: child,
  );
}
