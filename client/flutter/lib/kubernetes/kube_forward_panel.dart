import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_restore.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final kubeForwardsProvider = FutureProvider.autoDispose.family<List<KubeForward>, KubeScope>((ref, scope) async {
  if (!scope.unlocked) return const [];
  final service = ref.watch(kubeServiceProvider);
  return service is KubeForwardService ? (service as KubeForwardService).forwards() : const [];
});

Future<void> showKubeForwardDialog(
  BuildContext context, {
  required KubeSession session,
  required KubeResourceRef resource,
  required KubeForwardService service,
  required KubeResourceDetail detail,
  List<int>? ports,
}) => showAppDialog<void>(
  context,
  secure: true,
  builder: (_) => _ForwardDialog(session: session, resource: resource, service: service, detail: detail, ports: ports),
);

class _ForwardDialog extends ConsumerStatefulWidget {
  const _ForwardDialog({
    required this.session,
    required this.resource,
    required this.service,
    required this.detail,
    this.ports,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeForwardService service;
  final KubeResourceDetail detail;
  final List<int>? ports;
  @override
  ConsumerState<_ForwardDialog> createState() => _ForwardDialogState();
}

class _ForwardDialogState extends ConsumerState<_ForwardDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _remote = TextEditingController(), _local = TextEditingController(text: '0');
  final _address = TextEditingController(text: '0.0.0.0');
  bool _network = false, _approveNetwork = false;
  bool get _networkAllowed =>
      !AppPlatform.isMobile && !widget.session.production && widget.service is KubeNetworkForwardService;
  bool _busy = false, _failed = false, _restore = false;
  KubeForward? _created;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  List<int> get _ports =>
      widget.ports ??
            ((widget.detail.object['spec'] as Map?)?['containers'] as List? ?? const [])
                .whereType<Map<String, dynamic>>()
                .expand((container) => (container['ports'] as List? ?? const []).whereType<Map<String, dynamic>>())
                .where((port) => (port['protocol'] ?? 'TCP') == 'TCP')
                .map((port) => port['containerPort'])
                .whereType<int>()
                .where((port) => port > 0 && port <= 65535)
                .toSet()
                .toList()
        ..sort();
  @override
  void initState() {
    super.initState();
    if (_ports.isNotEmpty) _remote.text = _ports.first.toString();
  }

  @override
  void dispose() {
    _remote.dispose();
    _local.dispose();
    _address.dispose();
    super.dispose();
  }

  Future<void> _start() async {
    final remote = int.tryParse(_remote.text), local = int.tryParse(_local.text);
    if (!_current ||
        _busy ||
        !widget.session.canPortForward ||
        widget.resource.namespace == null ||
        remote == null ||
        remote < 1 ||
        remote > 65535 ||
        local == null ||
        local < 0 ||
        local > 65535 ||
        (_network &&
            (!_networkAllowed || !_approveNetwork || InternetAddress.tryParse(_address.text.trim()) == null))) {
      return;
    }
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final forward = _network
          ? await (widget.service as KubeNetworkForwardService).startNetworkForward(
              widget.session.id,
              namespace: widget.resource.namespace!,
              pod: widget.resource.name,
              remotePort: remote,
              localPort: local,
              localAddress: _address.text.trim(),
              approveNetwork: _approveNetwork,
            )
          : await widget.service.startForward(
              widget.session.id,
              namespace: widget.resource.namespace!,
              pod: widget.resource.name,
              remotePort: remote,
              localPort: local,
            );
      if (!_current) {
        await widget.service.stopForward(widget.session.id, forward.id);
        return;
      }
      if (_restore) {
        ref
            .read(kubeForwardRestoreProvider.notifier)
            .remember(
              KubeForwardRestorePlan(
                contextId: widget.session.contextId,
                namespace: widget.resource.namespace!,
                pod: widget.resource.name,
                remotePort: remote,
                localPort: int.tryParse(forward.address.split(':').last) ?? local,
                sessionId: forward.sessionId,
                forwardId: forward.id,
                localAddress: _network ? _address.text.trim() : '127.0.0.1',
                approveNetwork: _network && _approveNetwork,
              ),
            );
      }
      setState(() => _created = forward);
      ref.invalidate(kubeForwardsProvider(_scope));
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
      _remote.clear();
      _local.clear();
      _created = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final remote = int.tryParse(_remote.text), local = int.tryParse(_local.text);
    final valid =
        remote != null &&
        remote > 0 &&
        remote <= 65535 &&
        local != null &&
        local >= 0 &&
        local <= 65535 &&
        (!_network || (_networkAllowed && _approveNetwork && InternetAddress.tryParse(_address.text.trim()) != null));
    return GlassDialog(
      key: const ValueKey('kube-forward-dialog'),
      title: l.kubePortForward,
      content: !_current
          ? const SizedBox.shrink()
          : SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('${widget.resource.namespace}/${widget.resource.name}'),
                  Text(l.kubeForwardHelp),
                  if (!widget.session.canPortForward) Text(l.kubeForwardProductionBlocked),
                  if (_created case final forward?) ...[
                    SelectableText(forward.address, key: const ValueKey('kube-forward-address')),
                    Text(l.kubeForwardCreated),
                  ] else ...[
                    TextField(
                      key: const ValueKey('kube-forward-remote'),
                      controller: _remote,
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(5)],
                      enabled: !_busy,
                      decoration: InputDecoration(labelText: l.kubeRemotePort),
                      onChanged: (_) => setState(() {}),
                    ),
                    if (_ports.isNotEmpty)
                      Wrap(
                        spacing: 6,
                        children: _ports
                            .map(
                              (port) => ActionChip(
                                label: Text('$port'),
                                onPressed: _busy ? null : () => setState(() => _remote.text = '$port'),
                              ),
                            )
                            .toList(),
                      ),
                    TextField(
                      key: const ValueKey('kube-forward-local'),
                      controller: _local,
                      keyboardType: TextInputType.number,
                      inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(5)],
                      enabled: !_busy,
                      decoration: InputDecoration(labelText: l.kubeLocalPort, helperText: l.kubeLocalPortHelp),
                      onChanged: (_) => setState(() {}),
                    ),
                    if (_networkAllowed) ...[
                      SwitchListTile(
                        contentPadding: EdgeInsets.zero,
                        value: _network,
                        title: Text(l.kubeForwardNetwork),
                        onChanged: _busy
                            ? null
                            : (value) => setState(() {
                                _network = value;
                                _approveNetwork = false;
                              }),
                      ),
                      if (_network) ...[
                        TextField(
                          key: const ValueKey('kube-forward-bind-address'),
                          controller: _address,
                          autocorrect: false,
                          enableSuggestions: false,
                          enabled: !_busy,
                          decoration: InputDecoration(labelText: l.kubeForwardBindAddress),
                          onChanged: (_) => setState(() => _approveNetwork = false),
                        ),
                        CheckboxListTile(
                          key: const ValueKey('kube-forward-network-approve'),
                          contentPadding: EdgeInsets.zero,
                          value: _approveNetwork,
                          title: Text(l.kubeForwardNetworkWarning),
                          onChanged: _busy ? null : (value) => setState(() => _approveNetwork = value ?? false),
                        ),
                      ],
                    ],
                    CheckboxListTile(
                      key: const ValueKey('kube-forward-restore'),
                      contentPadding: EdgeInsets.zero,
                      value: _restore,
                      onChanged: _busy ? null : (value) => setState(() => _restore = value ?? false),
                      title: Text(l.kubeForwardRestore),
                      subtitle: Text(l.kubeForwardRestoreHelp),
                    ),
                    if (_failed)
                      Text(l.kubeForwardFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                    if (_busy) const LinearProgressIndicator(),
                  ],
                ],
              ),
            ),
      primaryAction: _created != null
          ? GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context))
          : GlassButton.prominent(
              key: const ValueKey('kube-forward-start'),
              label: l.kubeStartForward,
              onPressed: _current && valid && !_busy && widget.session.canPortForward ? _start : null,
            ),
      secondaryActions: _created != null
          ? const []
          : [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
    );
  }
}

/// Runtime-only forwards never become inventory items or survive profile lock.
class KubeForwardsPanel extends ConsumerStatefulWidget {
  const KubeForwardsPanel({this.sessionId, this.showEmpty = false, super.key});
  final String? sessionId;
  final bool showEmpty;
  @override
  ConsumerState<KubeForwardsPanel> createState() => _KubeForwardsPanelState();
}

class _KubeForwardsPanelState extends ConsumerState<KubeForwardsPanel> with WidgetsBindingObserver {
  Timer? _timer;
  bool _foreground = true;
  final _stopping = <String>{};
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _timer = Timer.periodic(const Duration(seconds: 3), (_) {
      if (!mounted || !_foreground) return;
      final scope = ref.read(kubeScopeProvider);
      if (scope.unlocked &&
          ref.read(kubeServiceProvider) is KubeForwardService &&
          !ref.read(kubeForwardsProvider(scope)).isLoading) {
        ref.invalidate(kubeForwardsProvider(scope));
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) => _foreground = state == AppLifecycleState.resumed;
  Future<void> _stop(KubeForward forward, KubeScope scope) async {
    final service = ref.read(kubeServiceProvider);
    if (!scope.unlocked || service is! KubeForwardService || _stopping.contains(forward.id)) return;
    setState(() => _stopping.add(forward.id));
    try {
      ref.read(kubeForwardRestoreProvider.notifier).forget(forward.id);
      await (service as KubeForwardService).stopForward(forward.sessionId, forward.id);
      if (mounted && identical(scope, ref.read(kubeScopeProvider))) ref.invalidate(kubeForwardsProvider(scope));
    } catch (_) {
      if (mounted && identical(scope, ref.read(kubeScopeProvider))) {
        showSnack(context, context.l10n.kubeRequestFailed, error: true);
      }
    } finally {
      if (mounted) setState(() => _stopping.remove(forward.id));
    }
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(kubeScopeProvider);
    if (!scope.unlocked || ref.watch(kubeServiceProvider) is! KubeForwardService) return const SizedBox.shrink();
    final result = ref.watch(kubeForwardsProvider(scope));
    final forwards = (result.value ?? const <KubeForward>[])
        .where((f) => widget.sessionId == null || f.sessionId == widget.sessionId)
        .toList();
    final l = context.l10n;
    if (forwards.isEmpty && !widget.showEmpty && !result.hasError) return const SizedBox.shrink();
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.kubePortForwards, style: Theme.of(context).textTheme.titleMedium),
        if (result.hasError) Text(l.kubeRequestFailed),
        if (forwards.isEmpty) Text(l.kubeNoForwards),
        if (forwards.isNotEmpty)
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 220),
            child: ListView.builder(
              shrinkWrap: true,
              itemCount: forwards.length,
              itemBuilder: (context, index) {
                final f = forwards[index];
                return ListTile(
                  key: ValueKey('kube-forward-${f.id}'),
                  title: Text('${f.address} → ${f.namespace}/${f.pod}:${f.remotePort}'),
                  subtitle: Text(
                    '${f.contextName} · ${switch (f.state) {
                      'running' => l.kubeForwardRunning,
                      'failed' => l.kubeForwardStopped,
                      _ => l.kubeForwardStopped,
                    }}\n${l.kubeForwardTraffic(f.activeConnections, formatBytes(l, f.bytesSent), formatBytes(l, f.bytesReceived))}',
                  ),
                  trailing: GlassIconButton(
                    icon: Icons.stop_circle_outlined,
                    tooltip: l.kubeStopForward,
                    onPressed: _stopping.contains(f.id) ? null : () => _stop(f, scope),
                  ),
                );
              },
            ),
          ),
      ],
    );
  }
}
