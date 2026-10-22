import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeBulkDialog extends ConsumerStatefulWidget {
  const KubeBulkDialog({
    required this.session,
    required this.contextName,
    required this.kind,
    required this.rows,
    required this.service,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final KubeResourceKind kind;
  final List<KubeResourceRow> rows;
  final KubeBrowserService service;
  @override
  ConsumerState<KubeBulkDialog> createState() => _KubeBulkDialogState();
}

class _KubeBulkDialogState extends ConsumerState<KubeBulkDialog> with VaultPassphraseCheck<KubeBulkDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _replicas = TextEditingController(text: '1');
  String _action = 'delete';
  final _completed = <String>{};
  String? _failed;
  bool _busy = false, _cancel = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _canWrite =>
      _current &&
      !widget.session.production &&
      (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
  bool get _workload =>
      widget.kind.group == 'apps' && const ['Deployment', 'StatefulSet', 'DaemonSet'].contains(widget.kind.kind);
  @override
  void dispose() {
    _replicas.dispose();
    super.dispose();
  }

  Future<void> _run() async {
    if (!_canWrite || _busy || passphraseBusy || !passphraseEntered || widget.rows.length > 100) {
      return;
    }
    final replicas = int.tryParse(_replicas.text);
    if (_action == 'scale' && (replicas == null || replicas < 0 || replicas > 1000000)) return;
    if (!await checkPassphrase() || !_canWrite || _busy) return;
    setState(() {
      _busy = true;
      _failed = null;
      _cancel = false;
    });
    try {
      for (final row in widget.rows) {
        if (!_canWrite || _cancel) break;
        if (_completed.contains(row.id)) continue;
        final reference = KubeResourceRef(api: widget.kind, name: row.name, namespace: row.namespace);
        if (_action == 'delete') {
          await (widget.service as KubeMutationService).deleteResource(
            widget.session.id,
            reference,
            uid: row.uid,
            resourceVersion: row.resourceVersion,
          );
        } else if (_action == 'restart') {
          await (widget.service as KubeOperationsService).restart(widget.session.id, reference, row.resourceVersion);
        } else {
          await (widget.service as KubeOperationsService).scale(
            widget.session.id,
            reference,
            row.resourceVersion,
            replicas!,
          );
        }
        if (_current) setState(() => _completed.add(row.id));
      }
    } catch (_) {
      if (_current) setState(() => _failed = context.l10n.kubeBulkFailed);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    ref.watch(kubeWriteAccessProvider(widget.session));
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _cancel = true;
      passphrase.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        title: l.kubeBulkActions,
        content: SizedBox(
          width: 720,
          height: 460,
          child: !_current
              ? const SizedBox.shrink()
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(l.kubeBulkHelp),
                    if (widget.session.production) Text(l.kubeBulkProductionBlocked),
                    GlassDropdownField<String>(
                      value: _action,
                      items: [
                        if (widget.service is KubeMutationService && widget.kind.verbs.contains('delete'))
                          GlassSelectItem(value: 'delete', label: l.commonDelete),
                        if (_workload && widget.service is KubeOperationsService)
                          GlassSelectItem(value: 'restart', label: l.kubeRestart),
                        if (_workload && widget.kind.kind != 'DaemonSet' && widget.service is KubeOperationsService)
                          GlassSelectItem(value: 'scale', label: l.kubeScale),
                      ],
                      onChanged: _busy || _completed.isNotEmpty ? null : (value) => setState(() => _action = value),
                    ),
                    if (_action == 'scale')
                      TextField(
                        controller: _replicas,
                        keyboardType: TextInputType.number,
                        enabled: !_busy,
                        decoration: InputDecoration(labelText: l.kubeReplicas),
                      ),
                    Expanded(
                      child: ListView.builder(
                        itemCount: widget.rows.length,
                        itemBuilder: (_, i) {
                          final row = widget.rows[i];
                          return ListTile(
                            leading: Icon(
                              _completed.contains(row.id) ? Icons.check_circle_outline : Icons.circle_outlined,
                            ),
                            title: Text(row.name),
                            subtitle: Text(
                              /* l10n-ignore: Namespace and immutable Kubernetes resourceVersion. */ '${row.namespace ?? ''} · ${row.resourceVersion}',
                            ), // l10n-ignore: Namespace and immutable Kubernetes resourceVersion.
                          );
                        },
                      ),
                    ),
                    Text('${_completed.length}/${widget.rows.length}'),
                    if (_failed != null) Text(_failed!),
                    SelectableText(widget.contextName, style: GlassTokens.of(context).typography.mono),
                    passphraseField(
                      key: const ValueKey('kube-bulk-confirm'),
                      enabled: !_busy,
                      onSubmitted: (_) => unawaited(_run()),
                    ),
                    if (_busy) const LinearProgressIndicator(),
                  ],
                ),
        ),
        primaryAction: GlassButton.destructive(
          key: const ValueKey('kube-bulk-run'),
          label: l.commonContinue,
          busy: passphraseBusy,
          onPressed:
              !_canWrite || _busy || passphraseBusy || !passphraseEntered || _completed.length == widget.rows.length
              ? null
              : () => unawaited(_run()),
        ),
        secondaryActions: [
          GlassButton(
            label: _busy ? l.commonCancel : l.commonClose,
            onPressed: () {
              if (_busy) {
                setState(() => _cancel = true);
              } else {
                closeDialog(context, _completed.isNotEmpty);
              }
            },
          ),
        ],
      ),
    );
  }
}
