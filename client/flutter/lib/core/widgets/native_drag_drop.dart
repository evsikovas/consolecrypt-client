import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';
import 'package:super_clipboard/super_clipboard.dart';
import 'package:super_drag_and_drop/super_drag_and_drop.dart';

/// Native drag and drop (`super_drag_and_drop`, MIT): the only file that
/// touches the plugin. On computers every drag is a system drag, so files
/// come in from Finder/Explorer and remote files go out to them; phones, the
/// web and `flutter test` (no native library) use Flutter's in-app drag.
final bool nativeDragDrop = !kIsWeb && !AppPlatform.isMobile && !Platform.environment.containsKey('FLUTTER_TEST');

/// Whether dragged files can be dropped into Finder/Explorer (file promises
/// on macOS, virtual files on Windows; Linux has neither).
bool get dragOutSupported => nativeDragDrop && (AppPlatform.isMacOS || AppPlatform.isWindows);

/// A file another application receives on drop. [fetch] stages it in a
/// temporary folder of its own (progress 0..1); the file is streamed to the
/// receiver and that folder deleted.
final class DragOutFile {
  const DragOutFile({required this.name, required this.fetch});

  final String name;
  final Future<File> Function(ValueChanged<double> progress) fetch;
}

// In-app payloads travel as a token: the platform channel only carries
// plain values. The last few drags are enough (one drag at a time).
const _payloadFormat = CustomValueFormat<int>(applicationId: 'net.evsikov.consolecrypt.drag');
final _payloads = <int, Object>{};
var _nextPayload = 0;

int _register(Object data) {
  _payloads[++_nextPayload] = data;
  _payloads.remove(_nextPayload - 8);
  return _nextPayload;
}

T? _payload<T extends Object>(DropSession session) {
  for (final item in session.items) {
    final data = _payloads[item.localData];
    if (data is T) return data;
  }
  return null;
}

bool _fromThisApp(DropSession session) => session.items.any((item) => item.localData != null);

const _anyFile = SimpleFileFormat(uniformTypeIdentifiers: ['public.data'], mimeTypes: ['application/octet-stream']);

/// Drags [data] to an [AppDropTarget] of the same type and, where
/// [dragOutSupported], hands the regular [files] (read at drag start) to
/// the application it is dropped on.
class AppDragSource<T extends Object> extends StatelessWidget {
  const AppDragSource({required this.data, required this.feedback, required this.child, this.files, super.key});

  final T data;
  final Widget feedback;
  final Widget child;
  final List<DragOutFile> Function()? files;

  @override
  Widget build(BuildContext context) {
    if (!nativeDragDrop) {
      return Draggable<T>(data: data, dragAnchorStrategy: pointerDragAnchorStrategy, feedback: feedback, child: child);
    }
    var out = const <DragOutFile>[];
    return DragItemWidget(
      allowedOperations: () => const [DropOperation.copy, DropOperation.move],
      dragBuilder: (context, _) => feedback,
      dragItemProvider: (_) {
        out = dragOutSupported ? (files?.call() ?? const []) : const [];
        final token = _register(data);
        final item = DragItem(localData: token, suggestedName: out.firstOrNull?.name)..add(_payloadFormat(token));
        if (out.isNotEmpty) _offer(item, out.first);
        return item;
      },
      child: DraggableWidget(
        // One system item per file; they share the first item's image.
        onDragConfiguration: (configuration, _) {
          final first = configuration.items.first;
          for (final file in out.skip(1)) {
            final item = DragItem(suggestedName: file.name);
            _offer(item, file);
            configuration.items.add(DragConfigurationItem(item: item, image: first.image.retain()));
          }
          return configuration;
        },
        child: child,
      ),
    );
  }
}

void _offer(DragItem item, DragOutFile file) => item.addVirtualFile(
  format: _anyFile,
  storageSuggestion: VirtualFileStorage.temporaryFile,
  provider: (sinkProvider, progress) => unawaited(_produce(file, sinkProvider, progress)),
);

Future<void> _produce(DragOutFile file, VirtualFileEventSinkProvider sinkProvider, WriteProgress progress) async {
  var cancelled = false;
  progress.onCancel.addListener(() => cancelled = true);
  File? staged;
  // Closed below, or by the plugin when the receiver cancels.
  // ignore: close_sinks
  EventSink<Uint8List>? sink;
  try {
    staged = await file.fetch((f) => progress.updateProgress(f * 0.9));
    final size = await staged.length();
    final out = sink = sinkProvider(fileSize: size) as EventSink<Uint8List>;
    var written = 0;
    await for (final chunk in staged.openRead()) {
      if (cancelled) return;
      out.add(chunk is Uint8List ? chunk : Uint8List.fromList(chunk));
      written += chunk.length;
      if (size > 0) progress.updateProgress(0.9 + 0.1 * written / size);
    }
    out.close();
  } catch (e) {
    if (!cancelled) (sink ?? sinkProvider(fileSize: 0)).addError(e);
  } finally {
    final folder = staged?.parent;
    if (folder != null && folder.existsSync()) await folder.delete(recursive: true);
  }
}

/// Accepts an [AppDragSource] of type [T] (in-app only).
class AppDropTarget<T extends Object> extends StatefulWidget {
  const AppDropTarget({required this.onAccept, required this.builder, this.canAccept, super.key});

  final ValueChanged<T> onAccept;
  final bool Function(T data)? canAccept;

  /// [hovering]: an acceptable drag is over the target.
  final Widget Function(BuildContext context, bool hovering) builder;

  @override
  State<AppDropTarget<T>> createState() => _AppDropTargetState<T>();
}

class _AppDropTargetState<T extends Object> extends State<AppDropTarget<T>> {
  bool _hovering = false;

  void _hover(bool value) {
    if (_hovering != value && mounted) setState(() => _hovering = value);
  }

  bool _accepts(T? data) => data != null && (widget.canAccept?.call(data) ?? true);

  @override
  Widget build(BuildContext context) {
    final target = DragTarget<T>(
      onWillAcceptWithDetails: (d) => _accepts(d.data),
      onAcceptWithDetails: (d) => widget.onAccept(d.data),
      builder: (context, candidates, _) => widget.builder(context, candidates.isNotEmpty || _hovering),
    );
    if (!nativeDragDrop) return target;
    return DropRegion(
      formats: const [_payloadFormat],
      onDropOver: (event) {
        final ok = _accepts(_payload<T>(event.session));
        _hover(ok);
        return ok ? DropOperation.move : DropOperation.none;
      },
      onDropLeave: (_) => _hover(false),
      onDropEnded: (_) => _hover(false),
      onPerformDrop: (event) async {
        _hover(false);
        final data = _payload<T>(event.session);
        if (_accepts(data)) widget.onAccept(data!);
      },
      child: target,
    );
  }
}

/// Files and folders dropped from Finder/Explorer (local paths, pointer
/// position in this widget).
class OsFileDropTarget extends StatelessWidget {
  const OsFileDropTarget({
    required this.child,
    required this.onDrop,
    this.onHover,
    this.onExit,
    this.enabled = true,
    super.key,
  });

  final Widget child;
  final void Function(List<String> paths, Offset localPosition) onDrop;
  final ValueChanged<Offset>? onHover;
  final VoidCallback? onExit;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    if (!nativeDragDrop || !enabled) return child;
    return DropRegion(
      formats: const [Formats.fileUri],
      hitTestBehavior: HitTestBehavior.opaque,
      onDropOver: (event) {
        final session = event.session;
        if (_fromThisApp(session) || !session.items.any((item) => item.canProvide(Formats.fileUri))) {
          return DropOperation.none;
        }
        onHover?.call(event.position.local);
        return DropOperation.copy;
      },
      onDropLeave: (_) => onExit?.call(),
      onPerformDrop: (event) async {
        onExit?.call();
        final paths = <String>[];
        for (final item in event.session.items) {
          final reader = item.dataReader;
          if (reader == null || !reader.canProvide(Formats.fileUri)) continue;
          final uri = await _fileUri(reader);
          if (uri != null && uri.isScheme('file')) paths.add(uri.toFilePath());
        }
        if (paths.isNotEmpty) onDrop(paths, event.position.local);
      },
      child: child,
    );
  }
}

Future<Uri?> _fileUri(DataReader reader) {
  final done = Completer<Uri?>();
  final progress = reader.getValue<Uri>(
    Formats.fileUri,
    (uri) => done.complete(uri),
    onError: (_) => done.complete(null),
  );
  if (progress == null) done.complete(null);
  return done.future;
}
