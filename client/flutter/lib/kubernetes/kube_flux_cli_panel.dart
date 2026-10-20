import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_preferences_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeFluxCliPanel extends ConsumerStatefulWidget {
  const KubeFluxCliPanel({
    required this.session,
    this.namespace,
    this.recommendedVersion,
    this.onTerminalOpened,
    this.onOutput,
    super.key,
  });
  final KubeSession session;
  final String? namespace, recommendedVersion;
  final ValueChanged<String>? onTerminalOpened;
  final void Function(String title, FluxCliOutput output)? onOutput;
  @override
  ConsumerState<KubeFluxCliPanel> createState() => _KubeFluxCliPanelState();
}

class _KubeFluxCliPanelState extends ConsumerState<KubeFluxCliPanel> {
  late final KubeScope _scope;
  late final KubeFluxCliService? _service;
  final _manual = TextEditingController(), _version = TextEditingController();
  final _name = TextEditingController(), _kind = TextEditingController(text: 'Deployment');
  final _directory = TextEditingController();
  FluxCliDiscovery? _discovery;
  FluxCliVersion? _installed;
  FluxPreferences _preferences = FluxPreferences();
  FluxPathAdvice? _pathAdvice;
  String? _controllerVersion;
  bool _later = false;
  KubeFluxPreferencesService? get _preferencesService =>
      _service is KubeFluxPreferencesService ? _service as KubeFluxPreferencesService : null;
  String? _path, _method, _ticket;
  String _action = 'check';
  bool _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _service = ref.read(kubeFluxCliServiceProvider);
    _version.text = widget.recommendedVersion ?? '2.9.6'; // l10n-ignore: pinned release version
    if (!AppPlatform.isMobile && _current) unawaited(_initialize());
  }

  @override
  void dispose() {
    _cancel();
    for (final controller in [_manual, _version, _name, _kind, _directory]) {
      controller.dispose();
    }
    _installed = null;
    _discovery = null;
    super.dispose();
  }

  void _cancel() {
    final ticket = _ticket;
    _ticket = null;
    if (ticket != null) unawaited(_service?.cancelFlux(ticket).catchError((Object _) {}));
  }

  Future<void> _work(Future<void> Function() operation) async {
    if (!_current || _busy || _service == null) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      await operation();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _initialize() async {
    try {
      final settings = await _preferencesService?.fluxPreferences(sessionId: widget.session.id);
      if (!_current) return;
      if (settings != null) {
        _preferences = settings.preferences;
        _manual.text = _preferences.manualPath ?? '';
        _method = _preferences.method;
        _controllerVersion = settings.controllerVersion;
        _pathAdvice = settings.path;
        _version.text = widget.recommendedVersion ?? _controllerVersion ?? _version.text;
      }
    } catch (_) {
      // Discovery remains useful when the cluster denies controller metadata.
    }
    if (_current) await _discover(remember: false);
  }

  String get _diffKey => '${widget.session.contextId}/${widget.namespace ?? 'flux-system'}/${_name.text.trim()}';
  Future<void> _remember() async {
    if (!_current) return;
    _preferences
      ..manualPath = _manual.text.trim().isEmpty ? null : _manual.text.trim()
      ..method = _method;
    if (_action == 'diff' && _name.text.trim().isNotEmpty && _directory.text.trim().isNotEmpty) {
      _preferences.diffDirectories[_diffKey] = _directory.text.trim();
      while (_preferences.diffDirectories.length > 200) {
        _preferences.diffDirectories.remove(_preferences.diffDirectories.keys.first);
      }
    }
    await _preferencesService?.saveFluxPreferences(_preferences);
  }

  Future<void> _discover({bool remember = true}) => _work(() async {
    final result = await _service!.discoverFlux(manualPath: _manual.text.trim().isEmpty ? null : _manual.text.trim());
    if (!_current) return;
    setState(() {
      _discovery = result;
      if (!result.candidates.contains(_path)) {
        _path = result.candidates.firstOrNull;
        _installed = null;
      }
      if (_method != 'official_release' && !result.methods.contains(_method)) {
        _method = result.methods.firstOrNull ?? 'official_release';
      }
    });
    if (remember) await _remember();
  });
  Future<void> _probe() => _work(() async {
    final path = _path;
    if (path == null) return;
    final version = await _service!.probeFlux(path);
    if (_current && _path == path) setState(() => _installed = version);
  });
  Future<void> _maintenance(String operation) => _work(() async {
    await _remember();
    if (!_current) return;
    final review = await _service!.reviewFlux(_version.text.trim(), _method ?? 'official_release', operation);
    if (!mounted || !_current) {
      await _service.cancelFlux(review.ticket);
      return;
    }
    _ticket = review.ticket;
    final confirmed = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => _ScopedFluxDialog(
        scope: _scope,
        title: context.l10n.kubeFluxCliConfirm,
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(context.l10n.kubeFluxCliVerify), const SizedBox(height: 16),
            SelectableText(
              '${_operationLabel(review.operation)} · ${review.version}\n${review.source}\n${review.destination}',
            ),
            if (review.executable != null) ...[
              const SizedBox(height: 12),
              SelectableText(jsonEncode([review.executable, ...review.arguments])),
            ],
            if (review.verifier != null) ...[
              const SizedBox(height: 12),
              SelectableText('Cosign: ${review.verifier}'), // l10n-ignore: Cosign tool name and executable path.
            ], // l10n-ignore: tool name
            if (review.sha256 != null) SelectableText('SHA-256: ${review.sha256}'), // l10n-ignore: algorithm
          ],
        ),
        confirm: true,
      ),
    );
    if (!_current || confirmed != true) {
      _cancel();
      return;
    }
    try {
      final terminal = await _service.approveFlux(review);
      if (!_current) return;
      if (terminal != null) widget.onTerminalOpened?.call(terminal);
      final result = await _service.discoverFlux(manualPath: _path);
      if (_current) {
        setState(() {
          _discovery = result;
          _path = result.candidates.firstOrNull;
          _installed = null;
        });
      }
    } finally {
      _cancel();
    }
  });
  Future<void> _run() => _work(() async {
    final path = _path;
    if (path == null) return;
    final action = <String, dynamic>{'action': _action};
    if (_action != 'check') action['namespace'] = widget.namespace ?? 'flux-system';
    if (['trace', 'tree', 'diff'].contains(_action)) action['name'] = _name.text.trim();
    if (_action == 'trace') action['kind'] = _kind.text.trim();
    if (_action == 'diff') action['path'] = _directory.text.trim();
    await _remember();
    if (!_current) return;
    final output = await _service!.runFlux(widget.session.id, path, action);
    if (!mounted || !_current) {
      output.dispose();
      return;
    }
    try {
      if (widget.onOutput != null) {
        widget.onOutput!('flux $_action', output); // l10n-ignore: CLI verb
        return;
      }
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (_) => _ScopedFluxDialog(
          scope: _scope,
          title: context.l10n.kubeFluxCliOutput,
          content: SelectableText('${output.stdout}\n${output.stderr}'),
        ),
      );
    } finally {
      output.dispose();
    }
  });
  Future<void> _terminal() => _work(() async {
    final path = _path;
    if (path == null) return;
    final id = await _service!.openFluxTerminal(widget.session.id, path);
    if (_current) widget.onTerminalOpened?.call(id);
  });
  Future<void> _hideAdvice() => _work(() async {
    _preferences.dismissAdvice = true;
    await _remember();
  });
  Future<void> _showPath() => _work(() async {
    final advice = _pathAdvice;
    if (advice == null || !mounted) return;
    final confirm = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => _ScopedFluxDialog(
        scope: _scope,
        title: context.l10n.kubeFluxCliPathHelp,
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(context.l10n.kubeFluxCliPathHint),
            const SizedBox(height: 12),
            SelectableText([advice.directory, ?advice.profileFile, ?advice.line].join('\n\n')),
          ],
        ),
        confirm: advice.canAdd,
      ),
    );
    if (!_current || confirm != true || !advice.canAdd) return;
    final updated = await _preferencesService?.addFluxUserPath();
    if (_current) setState(() => _pathAdvice = updated);
  });
  bool get _outdated {
    List<int>? parse(String? value) {
      final parts = value?.split('.').map(int.tryParse).toList();
      return parts?.length == 3 && parts!.every((part) => part != null) ? parts.cast<int>() : null;
    }

    final installed = parse(_installed?.version), cluster = parse(_controllerVersion);
    if (installed == null || cluster == null) return false;
    for (var i = 0; i < 3; i++) {
      if (installed[i] != cluster[i]) return installed[i] < cluster[i];
    }
    return false;
  }

  String _operationLabel(String operation) => switch (operation) {
    'update' => context.l10n.kubeFluxCliUpdate,
    'remove' => context.l10n.kubeFluxCliRemove,
    _ => context.l10n.kubeFluxCliInstall,
  };
  String _methodLabel(String method) => switch (method) {
    'homebrew' => 'Homebrew',
    'winget' => 'winget',
    'scoop' => 'Scoop',
    'chocolatey' => 'Chocolatey', // l10n-ignore: proper tool names
    _ => context.l10n.kubeFluxCliOfficial,
  };
  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(kubeScopeProvider);
    ref.listen(kubeScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        _cancel();
        _installed = null;
        _discovery = null;
      }
    });
    if (AppPlatform.isMobile || !identical(scope, _scope) || !_current || _service == null) {
      return const SizedBox.shrink();
    }
    final l10n = context.l10n;
    final methods = {...?_discovery?.methods, 'official_release'}.toList();
    final enabled = !_busy;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(l10n.kubeFluxCliTitle, style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(l10n.kubeFluxCliHelp),
            if (_controllerVersion != null) Text('${l10n.kubeFluxCliController}: $_controllerVersion'),
            if (!_preferences.dismissAdvice && !_later && _discovery != null && (_path == null || _outdated)) ...[
              const SizedBox(height: 12),
              Text(_path == null ? l10n.kubeFluxCliAdviceMissing : l10n.kubeFluxCliAdviceUpdate),
              Wrap(
                spacing: 8,
                children: [
                  TextButton(
                    onPressed: enabled ? () => _maintenance(_path == null ? 'install' : 'update') : null,
                    child: Text(_path == null ? l10n.kubeFluxCliInstall : l10n.kubeFluxCliUpdate),
                  ),
                  TextButton(
                    onPressed: enabled ? () => setState(() => _later = true) : null,
                    child: Text(l10n.kubeFluxCliLater),
                  ),
                  TextButton(onPressed: enabled ? _hideAdvice : null, child: Text(l10n.kubeFluxCliNeverSuggest)),
                ],
              ),
            ],
            if (_busy) const Padding(padding: EdgeInsets.symmetric(vertical: 8), child: LinearProgressIndicator()),
            if (_busy && _ticket != null) TextButton(onPressed: _cancel, child: Text(l10n.commonCancel)),
            if (_failed) Padding(padding: const EdgeInsets.symmetric(vertical: 8), child: Text(l10n.kubeFluxCliFailed)),
            const SizedBox(height: 12),
            TextField(
              key: const ValueKey('flux-manual-path'),
              controller: _manual,
              decoration: InputDecoration(labelText: l10n.kubeFluxCliPath),
              enabled: enabled,
            ),
            TextButton(
              key: const ValueKey('flux-find'),
              onPressed: enabled ? () => _discover() : null,
              child: Text(l10n.kubeFluxCliFind),
            ),
            if (_pathAdvice != null && !_pathAdvice!.present)
              TextButton(onPressed: enabled ? _showPath : null, child: Text(l10n.kubeFluxCliPathHelp)),
            if (_pathAdvice?.present == true) Text(l10n.kubeFluxCliPathAdded),
            if (_discovery?.candidates.isNotEmpty == true) ...[
              DropdownButtonFormField<String>(
                initialValue: _path,
                isExpanded: true,
                items: _discovery!.candidates
                    .map(
                      (path) => DropdownMenuItem(
                        value: path,
                        child: Text(path, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                    )
                    .toList(),
                onChanged: enabled
                    ? (path) => setState(() {
                        _path = path;
                        _installed = null;
                      })
                    : null,
              ),
              TextButton(
                key: const ValueKey('flux-probe'),
                onPressed: enabled ? _probe : null,
                child: Text(l10n.kubeFluxCliProbe),
              ),
              if (_installed != null)
                SelectableText('${_installed!.version}\nSHA-256: ${_installed!.sha256}'), // l10n-ignore: algorithm
            ],
            const SizedBox(height: 12),
            TextField(
              controller: _version,
              enabled: enabled,
              decoration: InputDecoration(labelText: l10n.kubeFluxCliVersion),
            ),
            DropdownButtonFormField<String>(
              initialValue: _method ?? 'official_release',
              isExpanded: true,
              decoration: InputDecoration(labelText: l10n.kubeFluxCliMethod),
              items: methods
                  .map(
                    (method) => DropdownMenuItem(
                      value: method,
                      child: Text(_methodLabel(method), overflow: TextOverflow.ellipsis),
                    ),
                  )
                  .toList(),
              onChanged: enabled ? (method) => setState(() => _method = method) : null,
            ),
            Wrap(
              spacing: 8,
              runSpacing: 4,
              children: [
                TextButton(
                  key: const ValueKey('flux-install'),
                  onPressed: enabled ? () => _maintenance('install') : null,
                  child: Text(l10n.kubeFluxCliInstall),
                ),
                if (_path != null) ...[
                  TextButton(
                    onPressed: enabled ? () => _maintenance('update') : null,
                    child: Text(l10n.kubeFluxCliUpdate),
                  ),
                  TextButton(
                    onPressed: enabled ? () => _maintenance('remove') : null,
                    child: Text(l10n.kubeFluxCliRemove),
                  ),
                ],
              ],
            ),
            if (_path != null) ...[
              const Divider(),
              DropdownButtonFormField<String>(
                initialValue: _action,
                isExpanded: true,
                items: ['check', 'trace', 'tree', 'events', 'logs', 'diff']
                    .map((action) => DropdownMenuItem(value: action, child: Text('flux $action')))
                    .toList(), // l10n-ignore: CLI verbs
                onChanged: enabled ? (action) => setState(() => _action = action!) : null,
              ),
              if (['trace', 'tree', 'diff'].contains(_action))
                TextField(
                  controller: _name,
                  enabled: enabled,
                  onChanged: (_) {
                    if (_action == 'diff') _directory.text = _preferences.diffDirectories[_diffKey] ?? '';
                  },
                  decoration: InputDecoration(labelText: l10n.kubeFluxCliResource),
                ),
              if (_action == 'trace')
                TextField(
                  controller: _kind,
                  enabled: enabled,
                  decoration: InputDecoration(labelText: l10n.kubeFluxCliKind),
                ),
              if (_action == 'diff')
                TextField(
                  controller: _directory,
                  enabled: enabled,
                  decoration: InputDecoration(labelText: l10n.kubeFluxCliLocalPath),
                ),
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  TextButton(
                    key: const ValueKey('flux-run'),
                    onPressed: enabled ? _run : null,
                    child: Text(l10n.kubeFluxCliRun),
                  ),
                  if (widget.onTerminalOpened != null)
                    TextButton(onPressed: enabled ? _terminal : null, child: Text(l10n.kubeFluxCliTerminal)),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _ScopedFluxDialog extends ConsumerWidget {
  const _ScopedFluxDialog({required this.scope, required this.title, required this.content, this.confirm = false});
  final KubeScope scope;
  final String title;
  final Widget content;
  final bool confirm;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (!identical(next, scope)) {
        final route = ModalRoute.of(context);
        if (route != null) Navigator.of(context).removeRoute(route);
      }
    });
    return AlertDialog(
      key: const ValueKey('flux-cli-dialog'),
      title: Text(title),
      content: SizedBox(
        width: 680,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .6),
          child: SingleChildScrollView(child: content),
        ),
      ),
      actions: [
        TextButton(
          autofocus: true,
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(confirm ? context.l10n.commonCancel : context.l10n.commonClose),
        ),
        if (confirm)
          TextButton(
            key: const ValueKey('flux-confirm'),
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.l10n.commonContinue),
          ),
      ],
    );
  }
}
