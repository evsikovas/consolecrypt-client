import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/kubernetes_sharing_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showSharingKubeContext(BuildContext context, {required String contextId}) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingKubePublishDialog(contextId: contextId));

class SharingKubePublishDialog extends ConsumerStatefulWidget {
  const SharingKubePublishDialog({required this.contextId, super.key});
  final String contextId;
  @override
  ConsumerState<SharingKubePublishDialog> createState() => _SharingKubePublishDialogState();
}

class _SharingKubePublishDialogState extends ConsumerState<SharingKubePublishDialog> {
  late final Object _scope = ref.read(sharingSessionScopeProvider);
  KubeSharingPreview? _preview;
  bool _credentials = false, _allowCredentials = false, _busy = false, _failed = false, _unsupported = false;
  List<SharingGrant> _grants = [];
  bool get _current => mounted && sharingSessionCurrent(ref, _scope);
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    if (!_current) return;
    setState(() {
      _busy = true;
      _failed = false;
      _preview = null;
    });
    try {
      final service = ref.read(sharingServiceProvider);
      final status = await service.status();
      if (!_current) return;
      if (!status.enabled || status.locked || !status.supportsKubeContexts || service is! KubernetesSharingService) {
        setState(() => _unsupported = true);
        return;
      }
      _allowCredentials = status.supportsSecrets;
      if (!_allowCredentials) _credentials = false;
      final preview = await (service as KubernetesSharingService).previewKubeContext(
        widget.contextId,
        includeCredentials: _credentials,
      );
      if (_current) {
        setState(() {
          _preview = preview;
          _unsupported = false;
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _publish() async {
    final preview = _preview;
    final service = ref.read(sharingServiceProvider);
    if (!_current || _busy || preview == null || _grants.isEmpty || service is! KubernetesSharingService) return;
    setState(() => _busy = true);
    final item = await runWithFeedback(
      context,
      () => (service as KubernetesSharingService).publishKubeContext(
        widget.contextId,
        List.of(_grants),
        expectedStamp: preview.snapshotStamp,
        includeCredentials: _credentials,
      ),
    );
    if (!mounted || !_current) return;
    if (item != null) {
      await runWithFeedback(context, service.flush);
      if (!mounted || !_current) return;
      refreshSharing(ref);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.sharingPublish,
        icon: Icons.share_outlined,
        content: SizedBox(
          width: 600,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_busy) const LinearProgressIndicator(),
                  if (_unsupported) Text(l.sharingKubeUnsupported),
                  if (_failed) ...[
                    Text(l.sharingUnavailable),
                    Text(l.sharingKubeSnapshotChanged),
                    GlassButton(label: l.commonRetry, onPressed: _busy ? null : _load),
                  ],
                  if (_preview case final preview?) ...[
                    Text(l.sharingPreview, style: Theme.of(context).textTheme.titleMedium),
                    const SizedBox(height: 8),
                    SharingProjectionView(preview.previewJson),
                    Text(l.sharingKubeExclude),
                    if (_allowCredentials)
                      CheckboxListTile(
                        key: const ValueKey('sharing-kube-include-credentials'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingKubeIncludeCredentials),
                        value: _credentials,
                        onChanged: _busy
                            ? null
                            : (value) {
                                _credentials = value == true;
                                _load();
                              },
                      ),
                    if (_credentials) Text(l.sharingKubeCredentialWarning),
                    const SizedBox(height: 16),
                    SharingRecipients(
                      key: const ValueKey('sharing-kube-recipients'),
                      initialGrants: _grants,
                      onChanged: (grants) => setState(() => _grants = grants),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-kube-publish-confirm'),
          label: l.sharingPublishConfirm,
          onPressed: _busy || _preview == null || _grants.isEmpty ? null : _publish,
        ),
      ),
    );
  }
}

Future<void> showSharingCopyKubeContext(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingKubeCopyDialog(item: item));

class SharingKubeCopyDialog extends ConsumerStatefulWidget {
  const SharingKubeCopyDialog({required this.item, super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingKubeCopyDialog> createState() => _SharingKubeCopyDialogState();
}

class _SharingKubeCopyDialogState extends ConsumerState<SharingKubeCopyDialog> {
  late final Object _scope = ref.read(sharingSessionScopeProvider);
  late String? _route = widget.item.data?['requires_route'] == true ? null : '';
  bool _credentials = false, _busy = false;
  bool get _current => mounted && sharingSessionCurrent(ref, _scope);
  Future<void> _copy() async {
    final service = ref.read(sharingServiceProvider);
    final route = _route;
    if (!_current ||
        _busy ||
        route == null ||
        widget.item.trust != SharingTrust.verified ||
        service is! KubernetesSharingService) {
      return;
    }
    final Map<String, dynamic> selected = route.startsWith('ssh:')
        ? {'kind': 'ssh', 'host_id': route.substring(4)}
        : route.startsWith('jump:')
        ? {'kind': 'jump', 'jump_profile_id': route.substring(5)}
        : {'kind': 'direct'};
    setState(() => _busy = true);
    final saved = await runWithFeedback(context, () async {
      await (service as KubernetesSharingService).copyKubeContext(
        widget.item.id,
        route: selected,
        includeCredentials: _credentials,
      );
      return true;
    }, success: context.l10n.sharingSaved);
    if (!mounted || !_current) return;
    if (saved == true) {
      ref.invalidate(kubeConnectionsProvider);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final hosts = ref.watch(hostsProvider).value?.where((host) => !host.isRdp).toList() ?? [];
    final jumps = ref.watch(jumpProfilesProvider).value ?? [];
    final requiresRoute = widget.item.data?['requires_route'] == true;
    final available = <String>{
      if (!requiresRoute) '',
      for (final host in hosts) 'ssh:${host.id.value}',
      for (final jump in jumps) 'jump:${jump.id.value}',
    };
    final allowed =
        widget.item.kind == SharingKind.kubeContext &&
        widget.item.trust == SharingTrust.verified &&
        widget.item.previewJson != null &&
        ref.watch(sharingServiceProvider) is KubernetesSharingService;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.sharingCopyPersonal,
        icon: Icons.hub_outlined,
        content: SizedBox(
          width: 560,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_busy) const LinearProgressIndicator(),
                  if (!allowed) Text(l.sharingUnavailable),
                  if (allowed) ...[
                    SharingProjectionView(widget.item.previewJson!),
                    Text(l.sharingKubeCopyHelp),
                    if (requiresRoute) Text(l.sharingKubeRouteRequired),
                    const SizedBox(height: 12),
                    DropdownButtonFormField<String>(
                      key: const ValueKey('sharing-kube-route'),
                      initialValue: available.contains(_route) ? _route : null,
                      isExpanded: true,
                      decoration: InputDecoration(labelText: l.kubeRoute),
                      items: [
                        if (!requiresRoute) DropdownMenuItem(value: '', child: Text(l.kubeRouteDirect)),
                        for (final host in hosts)
                          DropdownMenuItem(
                            value: 'ssh:${host.id.value}',
                            child: Text(
                              /* l10n-ignore: SSH protocol and saved host name. */ 'SSH · ${host.name}',
                              overflow: TextOverflow.ellipsis,
                            ), // l10n-ignore: SSH protocol and saved host name.
                          ),
                        for (final jump in jumps)
                          DropdownMenuItem(
                            value: 'jump:${jump.id.value}',
                            child: Text(jump.name, overflow: TextOverflow.ellipsis),
                          ),
                      ],
                      onChanged: _busy ? null : (value) => setState(() => _route = value),
                    ),
                    if (widget.item.data?['includes_credentials'] == true) ...[
                      CheckboxListTile(
                        key: const ValueKey('sharing-kube-import-credentials'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingKubeImportCredentials),
                        value: _credentials,
                        onChanged: _busy ? null : (value) => setState(() => _credentials = value == true),
                      ),
                      if (_credentials) Text(l.sharingKubeCredentialWarning),
                    ],
                  ],
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-kube-copy-confirm'),
          label: l.commonSave,
          onPressed: !allowed || _busy || !available.contains(_route) ? null : _copy,
        ),
      ),
    );
  }
}
