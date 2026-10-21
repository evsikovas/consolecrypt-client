import 'dart:async';
import 'dart:collection';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Lines kept in memory; older lines drop off the top.
const dockerLogLimit = 10000;

/// Live tail of one container: stderr in its own colour, timestamps,
/// pause, follow (stick to the end), find, copy and clear. Nothing is
/// written to disk. Logs are shown as they are, but copied text that holds
/// a secret (the core's masking rules) goes to the clipboard as a secret
/// and is cleared automatically.
class DockerLogsView extends ConsumerStatefulWidget {
  const DockerLogsView({
    required this.service,
    required this.sessionId,
    required this.containerId,
    this.active = true,
    super.key,
  });
  final DockerService service;
  final String sessionId, containerId;

  /// Streams only while the tab is shown.
  final bool active;
  @override
  ConsumerState<DockerLogsView> createState() => _DockerLogsViewState();
}

class _DockerLogsViewState extends ConsumerState<DockerLogsView> with WidgetsBindingObserver {
  final _lines = ListQueue<DockerLogLine>();
  final _scroll = ScrollController();
  final _find = TextEditingController();
  StreamSubscription<List<DockerLogLine>>? _subscription;
  List<DockerLogLine>? _frozen;
  bool _timestamps = false, _follow = true, _foreground = true, _failed = false, _streaming = false;
  int _generation = 0, _dropped = 0;

  bool get _paused => _frozen != null;

  /// Text selected in the lines (copy shortcuts and the context menu).
  String _selection = '';

  Future<void> _copy(String text) async {
    if (text.isEmpty) return;
    if (widget.service.containsSecret(text)) {
      await copySecretWithNotice(context, ref, text, what: context.l10n.copyWhatText);
    } else {
      await copyPlainWithNotice(context, ref, text);
    }
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (widget.active) _start();
  }

  @override
  void didUpdateWidget(covariant DockerLogsView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.containerId != widget.containerId || oldWidget.sessionId != widget.sessionId) {
      _restart();
    } else if (widget.active != oldWidget.active) {
      widget.active ? _start() : _stop();
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final foreground = state == AppLifecycleState.resumed;
    if (foreground == _foreground) return;
    _foreground = foreground;
    if (!foreground) {
      _stop();
    } else if (widget.active) {
      _start();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _stop();
    _lines.clear();
    _scroll.dispose();
    _find.dispose();
    super.dispose();
  }

  void _stop() {
    _generation++;
    unawaited(_subscription?.cancel());
    _subscription = null;
    _streaming = false;
  }

  /// A new stream replays the tail, so the buffer starts over.
  void _start() {
    if (_subscription != null || !_foreground) return;
    final generation = ++_generation;
    _lines.clear();
    _dropped = 0;
    _failed = false;
    _streaming = true;
    _subscription = widget.service
        .logs(widget.sessionId, widget.containerId, timestamps: _timestamps)
        .listen(
          (batch) {
            if (!mounted || generation != _generation) return;
            setState(() => _append(batch));
            _scrollToEnd();
          },
          onError: (Object _) {
            if (mounted && generation == _generation) {
              setState(() {
                _failed = true;
                _streaming = false;
              });
            }
          },
          onDone: () {
            if (mounted && generation == _generation) setState(() => _streaming = false);
          },
        );
    if (mounted) setState(() {});
  }

  void _restart() {
    _stop();
    if (widget.active) _start();
  }

  void _append(List<DockerLogLine> batch) {
    _lines.addAll(batch);
    while (_lines.length > dockerLogLimit) {
      _lines.removeFirst();
      _dropped++;
    }
  }

  void _scrollToEnd() {
    if (!_follow || _paused) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _scroll.hasClients) _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
  }

  String _render(DockerLogLine line) {
    final time = line.timestamp;
    return _timestamps && time != null ? '${time.toUtc().toIso8601String()} ${line.text}' : line.text;
  }

  List<DockerLogLine> get _visible {
    final source = _frozen ?? _lines.toList();
    final needle = _find.text.trim().toLowerCase();
    return needle.isEmpty ? source : source.where((line) => line.text.toLowerCase().contains(needle)).toList();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final visible = _visible;
    final searching = _find.text.trim().isNotEmpty;
    final mono = tokens.typography.mono.copyWith(fontSize: 12);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: 6,
          runSpacing: 6,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            GlassButton.plain(
              key: const ValueKey('docker-log-pause'),
              label: _paused ? l.dockerLogsResume : l.dockerLogsPause,
              icon: _paused ? Icons.play_arrow_rounded : Icons.pause_rounded,
              size: GlassControlSize.sm,
              onPressed: () => setState(() {
                _frozen = _paused ? null : _lines.toList();
                if (!_paused) _scrollToEnd();
              }),
            ),
            GlassButton.plain(
              key: const ValueKey('docker-log-follow'),
              label: l.dockerLogsFollow,
              icon: _follow ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
              size: GlassControlSize.sm,
              onPressed: () => setState(() {
                _follow = !_follow;
                _scrollToEnd();
              }),
            ),
            GlassButton.plain(
              key: const ValueKey('docker-log-timestamps'),
              label: l.dockerLogsTimestamps,
              icon: _timestamps ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
              size: GlassControlSize.sm,
              onPressed: () {
                setState(() {
                  _timestamps = !_timestamps;
                  _frozen = null;
                });
                _restart();
              },
            ),
            GlassIconButton(
              key: const ValueKey('docker-log-copy'),
              icon: Icons.content_copy_rounded,
              tooltip: l.dockerLogsCopy,
              size: 28,
              iconSize: 16,
              style: GlassIconButtonStyle.plain,
              onPressed: visible.isEmpty ? null : () => unawaited(_copy(visible.map(_render).join('\n'))),
            ),
            GlassIconButton(
              key: const ValueKey('docker-log-clear'),
              icon: Icons.delete_sweep_outlined,
              tooltip: l.dockerLogsClear,
              size: 28,
              iconSize: 16,
              style: GlassIconButtonStyle.plain,
              onPressed: () => setState(() {
                _lines.clear();
                _dropped = 0;
                if (_paused) _frozen = [];
              }),
            ),
          ],
        ),
        const SizedBox(height: 6),
        GlassField(
          key: const ValueKey('docker-log-find'),
          fieldKey: const ValueKey('docker-log-find-input'),
          controller: _find,
          search: true,
          placeholder: l.dockerLogsFind,
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 4),
        Text(
          [
            if (searching) l.dockerLogsMatches(visible.length),
            if (_paused) l.dockerLogsPaused,
            if (_dropped > 0) l.dockerLogsTrimmed(dockerLogLimit),
            if (_failed) l.dockerLogsFailed,
          ].join(' · '),
          key: const ValueKey('docker-log-status'),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: tokens.typography.caption.copyWith(color: _failed ? p.danger : tokens.secondaryLabel),
        ),
        const SizedBox(height: 4),
        Expanded(
          child: DecoratedBox(
            decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: GlassRadii.shape(tokens.radii.sm)),
            child: visible.isEmpty
                ? Center(
                    child: Text(
                      _streaming && !searching ? l.dockerLogsWaiting : l.dockerNoLogs,
                      style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                  )
                : Shortcuts(
                    shortcuts: const {
                      SingleActivator(LogicalKeyboardKey.keyC, meta: true): _CopyLogSelection(),
                      SingleActivator(LogicalKeyboardKey.keyC, control: true): _CopyLogSelection(),
                    },
                    child: Actions(
                      actions: {
                        _CopyLogSelection: CallbackAction<_CopyLogSelection>(
                          onInvoke: (_) => unawaited(_copy(_selection)),
                        ),
                      },
                      child: SelectionArea(
                        onSelectionChanged: (content) => _selection = content?.plainText ?? '',
                        contextMenuBuilder: (context, state) => AdaptiveTextSelectionToolbar.buttonItems(
                          anchors: state.contextMenuAnchors,
                          buttonItems: [
                            for (final item in state.contextMenuButtonItems)
                              if (item.type == ContextMenuButtonType.copy)
                                item.copyWith(
                                  onPressed: () {
                                    state.hideToolbar();
                                    unawaited(_copy(_selection));
                                  },
                                )
                              else
                                item,
                          ],
                        ),
                        child: ListView.builder(
                          key: const ValueKey('docker-log-lines'),
                          controller: _scroll,
                          padding: const EdgeInsets.all(8),
                          itemCount: visible.length,
                          itemBuilder: (_, index) {
                            final line = visible[index];
                            return Text(
                              _render(line),
                              key: ValueKey('docker-log-line-$index'),
                              style: mono.copyWith(color: line.stream == DockerLogStream.stderr ? p.danger : p.label),
                            );
                          },
                        ),
                      ),
                    ),
                  ),
          ),
        ),
      ],
    );
  }
}

/// Copy of the selected log text through the secret check.
class _CopyLogSelection extends Intent {
  const _CopyLogSelection();
}
