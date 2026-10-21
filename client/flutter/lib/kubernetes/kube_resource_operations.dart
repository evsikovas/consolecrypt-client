import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<bool> reviewKubeRestart(
  BuildContext context, {
  required KubeSession session,
  required KubeResourceRef resource,
  required KubeResourceDetail detail,
  required KubeOperationsService service,
}) async {
  final container = ProviderScope.containerOf(context, listen: false),
      scope = ProviderScope.containerOf(context, listen: false).read(kubeScopeProvider);
  final version = (detail.object['metadata'] as Map?)?['resourceVersion'] as String? ?? '';
  bool allowed() =>
      context.mounted &&
      scope.unlocked &&
      identical(scope, container.read(kubeScopeProvider)) &&
      (container.read(kubeWriteAccessProvider(session)).value?.canWrite ?? false);
  if (!allowed() ||
      version.isEmpty ||
      resource.api.group != 'apps' ||
      !const {'deployments', 'statefulsets', 'daemonsets'}.contains(resource.api.resource)) {
    return false;
  }
  final approved = await showTypedKubeConfirmation(
    context,
    expected: resource.name,
    title: context.l10n.kubeRestart,
    message: context.l10n.kubeMutationConfirmHelp,
    confirmLabel: context.l10n.kubeRestart,
  );
  if (!approved || !allowed()) return false;
  await service.restart(session.id, resource, version);
  return allowed();
}

class KubeResourceOperations extends ConsumerStatefulWidget {
  const KubeResourceOperations({
    required this.session,
    required this.resource,
    required this.detail,
    required this.service,
    required this.onChanged,
    super.key,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeResourceDetail detail;
  final KubeOperationsService service;
  final VoidCallback onChanged;
  @override
  ConsumerState<KubeResourceOperations> createState() => _KubeResourceOperationsState();
}

class _KubeResourceOperationsState extends ConsumerState<KubeResourceOperations> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  bool _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _canWrite => _current && (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
  String get _version => (widget.detail.object['metadata'] as Map?)?['resourceVersion'] as String? ?? '';

  Future<void> _action(String action, String label) async {
    if (!_canWrite || _busy || _version.isEmpty) return;
    final resource = widget.resource;
    if (action == 'restart') {
      setState(() {
        _busy = true;
        _failed = false;
      });
      try {
        final changed = await reviewKubeRestart(
          context,
          session: widget.session,
          resource: resource,
          detail: widget.detail,
          service: widget.service,
        );
        if (_current && changed) widget.onChanged();
      } catch (_) {
        if (_current) setState(() => _failed = true);
      } finally {
        if (_current) setState(() => _busy = false);
      }
      return;
    }
    if (action == 'scale' || action == 'debug' || action == 'drain') {
      final changed = await showAppDialog<bool>(
        context,
        secure: true,
        barrierDismissible: false,
        builder: (_) => _OperationDialog(
          session: widget.session,
          resource: resource,
          detail: widget.detail,
          service: widget.service,
          action: action,
        ),
      );
      if (_current && changed == true) widget.onChanged();
      return;
    }
    final approved = await showTypedKubeConfirmation(
      context,
      expected: resource.name,
      title: label,
      message: context.l10n.kubeMutationConfirmHelp,
      confirmLabel: label,
    );
    if (!_canWrite || !approved) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      await widget.service.cordon(widget.session.id, resource.name, _version, action == 'cordon');
      if (_current) widget.onChanged();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final api = widget.resource.api;
    final canWrite = ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false;
    final spec = widget.detail.object['spec'] as Map? ?? const {};
    final actions = <(String, String)>[
      if (api.group == 'apps' && const {'deployments', 'statefulsets', 'replicasets'}.contains(api.resource))
        ('scale', l.kubeScale),
      if (api.group == 'apps' && const {'deployments', 'statefulsets', 'daemonsets'}.contains(api.resource))
        ('restart', l.kubeRestart),
      if (api.group.isEmpty && api.resource == 'nodes') ...[
        spec['unschedulable'] == true ? ('uncordon', l.kubeUncordon) : ('cordon', l.kubeCordon),
        if (spec['unschedulable'] == true) ('drain', l.kubeDrain),
      ],
      if (api.isPod) ('debug', l.kubeDebugContainer),
    ];
    if (actions.isEmpty) return const SizedBox.shrink();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: actions
              .map(
                (action) => GlassButton(
                  key: ValueKey('kube-operation-${action.$1}'),
                  label: action.$2,
                  onPressed: !_busy && canWrite && _version.isNotEmpty ? () => _action(action.$1, action.$2) : null,
                ),
              )
              .toList(),
        ),
        if (_busy) const LinearProgressIndicator(),
        if (_failed) Text(l.kubeOperationFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
      ],
    );
  }
}

class _OperationDialog extends ConsumerStatefulWidget {
  const _OperationDialog({
    required this.session,
    required this.resource,
    required this.detail,
    required this.service,
    required this.action,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeResourceDetail detail;
  final KubeOperationsService service;
  final String action;
  @override
  ConsumerState<_OperationDialog> createState() => _OperationDialogState();
}

class _OperationDialogState extends ConsumerState<_OperationDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _confirm = TextEditingController(),
      _replicas = TextEditingController(),
      _image = TextEditingController(),
      _name = TextEditingController(text: 'debugger');
  String? _container;
  List<KubeDrainCandidate>? _plan;
  bool _loading = false, _busy = false, _failed = false, _changed = false;
  int _completed = 0;
  bool _stopRequested = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _canWrite => _current && (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
  String get _version => (widget.detail.object['metadata'] as Map?)?['resourceVersion'] as String? ?? '';
  @override
  void initState() {
    super.initState();
    _replicas.text = ((widget.detail.object['spec'] as Map?)?['replicas'] ?? 1).toString();
    _container = widget.detail.containers.firstOrNull;
    if (widget.action == 'drain') unawaited(_loadPlan());
  }

  @override
  void dispose() {
    _confirm.dispose();
    _replicas.dispose();
    _image.dispose();
    _name.dispose();
    super.dispose();
  }

  Future<void> _loadPlan() async {
    setState(() => _loading = true);
    try {
      final plan = await widget.service.drainPlan(widget.session.id, widget.resource.name);
      if (_current) setState(() => _plan = plan);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  bool get _valid {
    if (!_canWrite || _busy || _confirm.text != widget.resource.name || _version.isEmpty) return false;
    if (widget.action == 'scale') {
      final replicas = int.tryParse(_replicas.text);
      return replicas != null && replicas >= 0 && replicas <= 1000000;
    }
    if (widget.action == 'debug') {
      return RegExp(r'^[a-z0-9]([-a-z0-9]*[a-z0-9])?$').hasMatch(_name.text) &&
          _name.text.length <= 63 &&
          _image.text.trim().isNotEmpty &&
          !RegExp(r'\s').hasMatch(_image.text) &&
          _image.text.length <= 1024;
    }
    return _plan != null && !_loading && _plan!.isNotEmpty && _plan!.every((pod) => pod.blocker == null);
  }

  Future<void> _run() async {
    if (!_valid) return;
    setState(() {
      _busy = true;
      _failed = false;
      _stopRequested = false;
    });
    try {
      switch (widget.action) {
        case 'scale':
          await widget.service.scale(widget.session.id, widget.resource, _version, int.parse(_replicas.text));
        case 'debug':
          await widget.service.debugContainer(
            widget.session.id,
            widget.resource,
            _version,
            KubeDebugContainer(name: _name.text, image: _image.text, targetContainer: _container),
          );
        case 'drain':
          for (final pod in _plan!.skip(_completed)) {
            if (_stopRequested) return;
            if (!_canWrite) throw StateError('Write access revoked'); // l10n-ignore: diagnostic
            await widget.service.evictPod(widget.session.id, pod.reference, pod.uid, pod.resourceVersion);
            _changed = true;
            if (!_current) return;
            setState(() => _completed++);
          }
      }
      if (mounted && _current) closeDialog(context, true);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _plan = null;
      _confirm.clear();
      _image.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    ref.watch(kubeWriteAccessProvider(widget.session));
    final l = context.l10n;
    final title = switch (widget.action) {
      'scale' => l.kubeScale,
      'debug' => l.kubeDebugContainer,
      _ => l.kubeDrain,
    };
    return GlassDialog(
      title: title,
      width: 700,
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 520),
        child: !_current
            ? const SizedBox.shrink()
            : SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      /* l10n-ignore: Kubernetes kind, namespace and resource name. */ '${widget.resource.api.kind} · ${widget.resource.namespace ?? ''}/${widget.resource.name}',
                    ), // l10n-ignore: Kubernetes kind, namespace and resource name.
                    if (widget.action == 'scale')
                      TextField(
                        key: const ValueKey('kube-scale-replicas'),
                        controller: _replicas,
                        enabled: !_busy,
                        keyboardType: TextInputType.number,
                        inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(7)],
                        decoration: InputDecoration(labelText: l.kubeReplicas),
                        onChanged: (_) => setState(() {}),
                      ),
                    if (widget.action == 'debug') ...[
                      Text(l.kubeDebugHelp),
                      TextField(
                        key: const ValueKey('kube-debug-name'),
                        controller: _name,
                        enabled: !_busy,
                        decoration: InputDecoration(labelText: l.kubeName),
                        onChanged: (_) => setState(() {}),
                      ),
                      TextField(
                        key: const ValueKey('kube-debug-image'),
                        controller: _image,
                        enabled: !_busy,
                        autocorrect: false,
                        enableSuggestions: false,
                        decoration: InputDecoration(labelText: l.kubeContainerImage),
                        onChanged: (_) => setState(() {}),
                      ),
                      if (widget.detail.containers.isNotEmpty)
                        GlassDropdownField<String>(
                          value: _container,
                          label: l.kubeContainer,
                          items: [
                            for (final name in widget.detail.containers) GlassSelectItem(value: name, label: name),
                          ],
                          onChanged: _busy ? null : (value) => setState(() => _container = value),
                        ),
                    ],
                    if (widget.action == 'drain') ...[
                      Text(l.kubeDrainHelp),
                      if (_plan != null) Text(l.kubeDrainProgress(_completed, _plan!.length)),
                      if (_plan?.isEmpty == true) Text(l.kubeDrainEmpty),
                      if (_plan?.isNotEmpty == true)
                        SizedBox(
                          height: 180,
                          child: ListView.builder(
                            itemCount: _plan!.length,
                            itemBuilder: (context, index) {
                              final pod = _plan![index];
                              return ListTile(
                                title: Text('${pod.reference.namespace}/${pod.reference.name}'),
                                subtitle: pod.blocker == null
                                    ? null
                                    : Text(switch (pod.blocker) {
                                        'unmanaged_pod' => l.kubeDrainUnmanaged,
                                        'local_empty_dir_data' => l.kubeDrainLocalData,
                                        _ => l.kubeDrainBlocked,
                                      }),
                              );
                            },
                          ),
                        ),
                    ],
                    Text(l.kubeMutationConfirmHelp),
                    TextField(
                      key: const ValueKey('kube-operation-confirm-name'),
                      controller: _confirm,
                      enabled: !_busy,
                      autocorrect: false,
                      enableSuggestions: false,
                      enableIMEPersonalizedLearning: false,
                      decoration: InputDecoration(labelText: widget.resource.name),
                      onChanged: (_) => setState(() {}),
                    ),
                    if (_loading || _busy) const LinearProgressIndicator(),
                    if (_failed)
                      Text(l.kubeOperationFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  ],
                ),
              ),
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('kube-operation-confirm'),
        label: title,
        onPressed: _valid ? _run : null,
      ),
      secondaryActions: [
        if (_busy && widget.action == 'drain')
          GlassButton(label: l.commonCancel, onPressed: () => setState(() => _stopRequested = true))
        else
          GlassButton(label: l.commonClose, onPressed: _busy ? null : () => closeDialog(context, _changed)),
      ],
    );
  }
}
