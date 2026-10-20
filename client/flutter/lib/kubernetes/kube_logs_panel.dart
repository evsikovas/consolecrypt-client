import 'dart:async';
import 'dart:collection';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_log_filter.dart';
import 'package:consolecrypt/kubernetes/kube_logs_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeLogsPanel extends ConsumerStatefulWidget {
  const KubeLogsPanel({
    required this.session,
    required this.resource,
    required this.service,
    this.containers = const [],
    this.active = true,
    this.initialFilter = '',
    this.initialSinceSeconds,
    super.key,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeBrowserService service;
  final List<String> containers;
  final bool active;
  final String initialFilter;
  final int? initialSinceSeconds;
  @override
  ConsumerState<KubeLogsPanel> createState() => _KubeLogsPanelState();
}

class _KubeLogsPanelState extends ConsumerState<KubeLogsPanel> with WidgetsBindingObserver {
  late final KubeScope _scope;
  final _lines = ListQueue<({String text, int bytes})>();
  final _filter = KubeLogFilter();
  final _search = TextEditingController();
  final _subscriptions = <StreamSubscription<String>>[];
  Timer? _debounce;
  List<String> _visible = [];
  KubeLogTargets? _targets;
  String? _pod, _container;
  String _level = 'all';
  int _bytes = 0, _generation = 0, _filterGeneration = 0, _since = 0;
  bool _follow = false, _previous = false, _wrap = true, _regex = false;
  bool _paused = false, _foreground = true, _busy = false, _failed = false, _badPattern = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  KubeAdvancedLogService? get _advanced =>
      widget.service is KubeAdvancedLogService ? widget.service as KubeAdvancedLogService : null;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _search.text = widget.initialFilter;
    _since = (widget.initialSinceSeconds ?? 0).clamp(0, 7 * 24 * 60 * 60);
    WidgetsBinding.instance.addObserver(this);
    if (widget.active) unawaited(_load());
  }

  @override
  void didUpdateWidget(covariant KubeLogsPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!widget.active) {
      unawaited(_stop());
      _busy = false;
    } else if (!oldWidget.active && !_paused) {
      unawaited(_load());
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
    if (!_foreground) {
      unawaited(_stop());
      if (mounted) setState(() => _busy = false);
    } else if (widget.active && !_paused && _follow) {
      unawaited(_load());
    }
  }

  Future<void> _stop() async {
    _generation++;
    final old = List.of(_subscriptions);
    _subscriptions.clear();
    await Future.wait(old.map((subscription) => subscription.cancel()));
  }

  void _clear() {
    _filterGeneration++;
    _debounce?.cancel();
    _filter.cancel();
    _lines.clear();
    _visible = [];
    _bytes = 0;
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    unawaited(_stop());
    _clear();
    _targets = null;
    _search.dispose();
    super.dispose();
  }

  KubeResourceRef _reference(String pod) => KubeResourceRef(
    api: const KubeResourceKind(group: '', version: 'v1', resource: 'pods', kind: 'Pod', namespaced: true),
    namespace: widget.resource.namespace,
    name: pod,
  );
  Future<void> _load() async {
    if (!_current || !widget.active || !_foreground || _paused) return;
    final stopping = _stop();
    final generation = _generation;
    await stopping;
    if (!_current || generation != _generation || !widget.active || !_foreground || _paused) return;
    setState(() {
      _busy = true;
      _failed = false;
      _clear();
    });
    try {
      final targets =
          await _advanced?.logTargets(widget.session.id, widget.resource) ??
          KubeLogTargets(
            targets: widget.resource.api.isPod
                ? [KubeLogTarget(pod: widget.resource.name, containers: widget.containers)]
                : [],
          );
      if (!_current || generation != _generation) return;
      setState(() {
        _targets = targets;
        if (_pod != null && !targets.targets.any((target) => target.pod == _pod)) _pod = null;
        if (_container != null && !targets.targets.any((target) => target.containers.contains(_container))) {
          _container = null;
        }
      });
      final selected = targets.targets
          .where(
            (target) =>
                (_pod == null || target.pod == _pod) && (_container == null || target.containers.contains(_container)),
          )
          .take(8)
          .toList();
      await Future.wait(
        selected.map((target) async {
          final container = _container ?? target.containers.firstOrNull;
          final reference = _reference(target.pod);
          final prefix = targets.targets.length > 1 ? '[${target.pod}/${container ?? ''}] ' : '';
          if (_follow && !_previous && (_advanced != null || widget.service is KubeLogService)) {
            final source =
                _advanced?.followLogsSince(
                  widget.session.id,
                  reference,
                  container: container,
                  tailLines: 500,
                  sinceSeconds: _since == 0 ? null : _since,
                ) ??
                (widget.service as KubeLogService).followLogs(widget.session.id, reference, container: container);
            final subscription = source.listen(
              (chunk) {
                if (_current && generation == _generation) {
                  _append(chunk, prefix);
                }
              },
              onError: (Object _) {
                if (_current && generation == _generation) setState(() => _failed = true);
              },
            );
            if (!_current || generation != _generation) {
              await subscription.cancel();
              return;
            }
            _subscriptions.add(subscription);
            return;
          }
          try {
            final text =
                await (_advanced?.logsSince(
                      widget.session.id,
                      reference,
                      container: container,
                      previous: _previous,
                      tailLines: 1000,
                      sinceSeconds: _since == 0 ? null : _since,
                    ) ??
                    widget.service.logs(widget.session.id, reference, container: container, previous: _previous));
            if (_current && generation == _generation) _append(text, prefix);
          } catch (_) {
            if (_current && generation == _generation) setState(() => _failed = true);
          }
        }),
      );
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  void _append(String chunk, String prefix) {
    // Native snapshots are bounded. Keep a second byte-and-line bound in UI
    // even for alternative adapters; no persistent log cache is created.
    if (chunk.length > 2 * 1024 * 1024) chunk = chunk.substring(chunk.length - 2 * 1024 * 1024);
    final parts = chunk.split('\n');
    if (parts.last.isEmpty) parts.removeLast();
    for (final part in parts) {
      final text = '$prefix$part';
      final bytes = utf8.encode(text).length + 1;
      if (bytes > 2 * 1024 * 1024) continue;
      _lines.add((text: text, bytes: bytes));
      _bytes += bytes;
      while (_lines.length > 20000 || _bytes > 2 * 1024 * 1024) {
        _bytes -= _lines.removeFirst().bytes;
      }
    }
    _scheduleFilter();
  }

  void _scheduleFilter() {
    if (_debounce?.isActive == true) return;
    _debounce = Timer(const Duration(milliseconds: 80), () => unawaited(_applyFilter()));
  }

  Future<void> _applyFilter() async {
    final generation = ++_filterGeneration;
    try {
      final visible = await _filter.apply(_lines.map((line) => line.text).toList(), _search.text, _regex, _level);
      if (_current && generation == _filterGeneration) {
        setState(() {
          _visible = visible;
          _badPattern = false;
        });
      }
    } catch (_) {
      if (_current && generation == _filterGeneration) {
        setState(() {
          _visible = [];
          _badPattern = true;
        });
      }
    }
  }

  void _configure(VoidCallback change) {
    setState(change);
    unawaited(_load());
  }

  Future<void> _export() async {
    final service = _advanced;
    if (service == null || !_current || _visible.isEmpty) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (_) =>
          _LogExportDialog(scope: _scope, session: widget.session, service: service, contents: _visible.join('\n')),
    );
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(kubeScopeProvider);
    ref.listen(kubeScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        unawaited(_stop());
        _clear();
        _targets = null;
      }
    });
    if (!identical(scope, _scope) || !_current) return const SizedBox.shrink();
    final l = context.l10n;
    final targets = _targets?.targets ?? [];
    final containers = targets
        .where((target) => _pod == null || target.pod == _pod)
        .expand((target) => target.containers)
        .toSet()
        .toList();
    final canFollow = _advanced != null || widget.service is KubeLogService;
    return LayoutBuilder(
      builder: (context, constraints) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ConstrainedBox(
            constraints: BoxConstraints(maxHeight: constraints.maxHeight * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      if (targets.length > 1)
                        SizedBox(
                          width: 230,
                          child: DropdownButtonFormField<String>(
                            key: ValueKey(('log-pods', _pod)),
                            initialValue: _pod ?? '',
                            isExpanded: true,
                            items: [
                              DropdownMenuItem(value: '', child: Text(l.kubeLogAllPods)),
                              ...targets.map(
                                (target) => DropdownMenuItem(
                                  value: target.pod,
                                  child: Text(target.pod, overflow: TextOverflow.ellipsis),
                                ),
                              ),
                            ],
                            onChanged: (value) => _configure(() {
                              _pod = value?.isEmpty == true ? null : value;
                              _container = null;
                            }),
                          ),
                        ),
                      if (containers.isNotEmpty)
                        SizedBox(
                          width: 190,
                          child: DropdownButtonFormField<String>(
                            key: ValueKey(('log-containers', _container, containers.join(','))),
                            initialValue: _container ?? '',
                            isExpanded: true,
                            decoration: InputDecoration(labelText: l.kubeContainer),
                            items: [
                              DropdownMenuItem(value: '', child: Text(l.kubeLogDefaultContainer)),
                              ...containers.map(
                                (name) => DropdownMenuItem(
                                  value: name,
                                  child: Text(name, overflow: TextOverflow.ellipsis),
                                ),
                              ),
                            ],
                            onChanged: (value) => _configure(() => _container = value?.isEmpty == true ? null : value),
                          ),
                        ),
                      if (_advanced != null)
                        SizedBox(
                          width: 190,
                          child: DropdownButtonFormField<int>(
                            initialValue: _since,
                            decoration: InputDecoration(labelText: l.kubeLogSince),
                            isExpanded: true,
                            items: [
                              DropdownMenuItem(value: 0, child: Text(l.kubeLogAllTime)),
                              DropdownMenuItem(value: 60, child: Text(l.kubeLogLastMinute)),
                              DropdownMenuItem(value: 3600, child: Text(l.kubeLogLastHour)),
                              DropdownMenuItem(value: 86400, child: Text(l.kubeLogLastDay)),
                              if (!const {0, 60, 3600, 86400}.contains(_since))
                                DropdownMenuItem(
                                  value: _since,
                                  child: Text(Duration(seconds: _since).toString().split('.').first),
                                ),
                            ],
                            onChanged: (value) => _configure(() => _since = value ?? 0),
                          ),
                        ),
                      TextButton(onPressed: _busy ? null : _load, child: Text(l.kubeRefreshDetails)),
                    ],
                  ),
                  Wrap(
                    spacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      FilterChip(
                        key: const ValueKey('kube-log-previous'),
                        label: Text(l.kubePreviousLogs),
                        selected: _previous,
                        onSelected: (value) => _configure(() {
                          _previous = value;
                          if (value) _follow = false;
                        }),
                      ),
                      if (canFollow)
                        FilterChip(
                          key: const ValueKey('kube-log-follow'),
                          label: Text(l.kubeFollowLogs),
                          selected: _follow,
                          onSelected: (value) => _configure(() {
                            _follow = value;
                            _paused = false;
                            if (value) _previous = false;
                          }),
                        ),
                      if (_follow)
                        TextButton(
                          key: const ValueKey('kube-log-pause'),
                          onPressed: () {
                            setState(() {
                              _paused = !_paused;
                              if (_paused) _busy = false;
                            });
                            if (_paused) {
                              unawaited(_stop());
                            } else {
                              unawaited(_load());
                            }
                          },
                          child: Text(_paused ? l.kubeResumeLogs : l.kubePauseLogs),
                        ),
                      FilterChip(
                        label: Text(l.kubeLogWrap),
                        selected: _wrap,
                        onSelected: (value) => setState(() => _wrap = value),
                      ),
                      if (_advanced != null)
                        TextButton(onPressed: _visible.isEmpty ? null : _export, child: Text(l.kubeLogExport)),
                    ],
                  ),
                  TextField(
                    key: const ValueKey('kube-log-filter'),
                    controller: _search,
                    decoration: InputDecoration(labelText: l.kubeLogFilter),
                    onChanged: (_) => _scheduleFilter(),
                  ),
                  Wrap(
                    spacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      FilterChip(
                        label: Text(l.kubeLogRegex),
                        selected: _regex,
                        onSelected: (value) {
                          setState(() => _regex = value);
                          _scheduleFilter();
                        },
                      ),
                      SizedBox(
                        width: 170,
                        child: DropdownButton<String>(
                          value: _level,
                          isExpanded: true,
                          items: ['all', 'error', 'warn', 'info', 'debug']
                              .map(
                                (level) => DropdownMenuItem(
                                  value: level,
                                  child: Text(
                                    level == 'all' ? l.kubeLogAllLevels : level.toUpperCase(),
                                  ), // l10n-ignore: log severity values
                                ),
                              )
                              .toList(),
                          onChanged: (level) {
                            setState(() => _level = level ?? 'all');
                            _scheduleFilter();
                          },
                        ),
                      ),
                    ],
                  ),
                  Text(l.kubeLogBufferHelp, style: Theme.of(context).textTheme.bodySmall),
                  if (_targets?.truncated == true) Text(l.kubeLogTargetsTruncated),
                  if (_badPattern) Text(l.kubeLogRegexInvalid),
                  if (_failed) Text(l.kubeRequestFailed),
                  if (_busy) const LinearProgressIndicator(),
                ],
              ),
            ),
          ),
          const SizedBox(height: 8),
          Expanded(
            child: _visible.isEmpty
                ? Center(child: Text(l.kubeNoLogs))
                : ListView.builder(
                    key: const ValueKey('kube-log-lines'),
                    itemCount: _visible.length,
                    itemBuilder: (_, index) {
                      final text = SelectableText(
                        _visible[index],
                        style: const TextStyle(fontFamily: 'monospace', fontSize: 12), // l10n-ignore: font family
                        maxLines: _wrap ? null : 1,
                      );
                      return _wrap ? text : SingleChildScrollView(scrollDirection: Axis.horizontal, child: text);
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

class _LogExportDialog extends ConsumerStatefulWidget {
  const _LogExportDialog({required this.scope, required this.session, required this.service, required this.contents});
  final KubeScope scope;
  final KubeSession session;
  final KubeAdvancedLogService service;
  final String contents;
  @override
  ConsumerState<_LogExportDialog> createState() => _LogExportDialogState();
}

class _LogExportDialogState extends ConsumerState<_LogExportDialog> {
  bool _mask = true, _busy = false, _failed = false;
  bool get _current => mounted && widget.scope.unlocked && identical(widget.scope, ref.read(kubeScopeProvider));
  Future<void> _save() async {
    if (!_current || _busy) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    String? path;
    Uint8List? bytes;
    try {
      final files = ref.read(fileDialogServiceProvider);
      path = await files.chooseSaveFile(suggestedName: 'kubernetes.log', extensions: ['log', 'txt']);
      if (path == null || !_current) return;
      bytes = Uint8List.fromList(utf8.encode(widget.contents));
      await widget.service.exportLogs(widget.session.id, path, bytes, mask: _mask);
      if (!_current) return;
      final saved = await files.finishSaveFile(path);
      if (mounted && _current && saved) {
        showSnack(context, context.l10n.kubeLogSaved);
        Navigator.of(context).pop();
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
      if (path != null && AppPlatform.isMobile) {
        try {
          await File(path).delete();
        } catch (_) {}
      }
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (!identical(widget.scope, next)) {
        final route = ModalRoute.of(context);
        if (route != null) Navigator.of(context).removeRoute(route);
      }
    });
    return AlertDialog(
      title: Text(context.l10n.kubeLogExport),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(context.l10n.kubeLogExportHelp),
          CheckboxListTile(
            value: _mask,
            onChanged: _busy ? null : (value) => setState(() => _mask = value == true),
            title: Text(context.l10n.kubeLogMask),
          ),
          if (_busy) const LinearProgressIndicator(),
          if (_failed) Text(context.l10n.kubeRequestFailed),
        ],
      ),
      actions: [
        TextButton(onPressed: _busy ? null : () => Navigator.of(context).pop(), child: Text(context.l10n.commonCancel)),
        TextButton(onPressed: _busy ? null : _save, child: Text(context.l10n.commonSave)),
      ],
    );
  }
}
