import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/rdp_sharing_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shares a saved RDP host card (FEAT-05): address, port, user, domain and
/// tags; notes and the saved password only when ticked.
Future<void> showSharingRdpHost(BuildContext context, {required String hostId}) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingRdpPublishDialog(hostId: hostId));

class SharingRdpPublishDialog extends ConsumerStatefulWidget {
  const SharingRdpPublishDialog({required this.hostId, super.key});
  final String hostId;
  @override
  ConsumerState<SharingRdpPublishDialog> createState() => _SharingRdpPublishDialogState();
}

class _SharingRdpPublishDialogState extends ConsumerState<SharingRdpPublishDialog> {
  late final Object _scope = ref.read(sharingSessionScopeProvider);
  RdpSharingPreview? _preview;
  bool _password = false, _notes = false, _allowPassword = false, _busy = false, _failed = false, _unsupported = false;
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
      if (!status.enabled || status.locked || service is! RdpSharingService) {
        setState(() => _unsupported = true);
        return;
      }
      _allowPassword = status.supportsSecrets;
      if (!_allowPassword) _password = false;
      final preview = await (service as RdpSharingService).previewRdpHost(
        widget.hostId,
        includePassword: _password,
        includeNotes: _notes,
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
    if (!_current || _busy || preview == null || _grants.isEmpty || service is! RdpSharingService) return;
    setState(() => _busy = true);
    final item = await runWithFeedback(
      context,
      () => (service as RdpSharingService).publishRdpHost(
        widget.hostId,
        List.of(_grants),
        expectedStamp: preview.snapshotStamp,
        includePassword: _password,
        includeNotes: _notes,
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
    final preview = _preview;
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
                  if (_unsupported) Text(l.sharingUnavailable, key: const ValueKey('sharing-rdp-unsupported')),
                  if (_failed) ...[
                    Text(l.sharingUnavailable),
                    GlassButton(label: l.commonRetry, onPressed: _busy ? null : _load),
                  ],
                  if (preview != null) ...[
                    Text(l.sharingPreview, style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 8),
                    SharingProjectionView(preview.previewJson),
                    Text(l.sharingRdpHelp),
                    CheckboxListTile(
                      key: const ValueKey('sharing-rdp-include-notes'),
                      contentPadding: EdgeInsets.zero,
                      title: Text(l.sharingIncludeNotes),
                      value: _notes,
                      onChanged: _busy
                          ? null
                          : (value) {
                              _notes = value == true;
                              _load();
                            },
                    ),
                    if (_allowPassword)
                      CheckboxListTile(
                        key: const ValueKey('sharing-rdp-include-password'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingRdpIncludePassword),
                        value: _password,
                        onChanged: _busy
                            ? null
                            : (value) {
                                _password = value == true;
                                _load();
                              },
                      ),
                    if (_password) Text(l.sharingRdpPasswordWarning),
                    const SizedBox(height: 16),
                    SharingRecipients(
                      key: const ValueKey('sharing-rdp-recipients'),
                      sensitive: _password,
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
          key: const ValueKey('sharing-rdp-publish-confirm'),
          label: l.sharingPublishConfirm,
          onPressed: _busy || preview == null || _grants.isEmpty ? null : _publish,
        ),
      ),
    );
  }
}

/// Saves a shared RDP host card as one's own RDP host; a shared password
/// is imported only when ticked.
Future<void> showSharingCopyRdpHost(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => _SharingCopyRdpHost(item));

class _SharingCopyRdpHost extends ConsumerStatefulWidget {
  const _SharingCopyRdpHost(this.item);
  final SharingItem item;
  @override
  ConsumerState<_SharingCopyRdpHost> createState() => _SharingCopyRdpHostState();
}

class _SharingCopyRdpHostState extends ConsumerState<_SharingCopyRdpHost> {
  late final Object? _profile = ref.read(sharingSessionScopeProvider);
  bool _password = false, _busy = false;

  Future<void> _save() async {
    final service = ref.read(sharingServiceProvider);
    if (!sharingSessionCurrent(ref, _profile) || service is! RdpSharingService) return;
    setState(() => _busy = true);
    final saved = await runWithFeedback(context, () async {
      await (service as RdpSharingService).copyRdpHost(widget.item.id, includePassword: _password);
      return true;
    }, success: context.l10n.sharingSaved);
    if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
    if (saved != null) {
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final hasPassword = widget.item.data?['includes_password'] == true;
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        title: l.sharingCopyPersonal,
        content: SizedBox(
          width: 520,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (widget.item.previewJson case final preview?) SharingProjectionView(preview),
              if (hasPassword)
                CheckboxListTile(
                  key: const ValueKey('sharing-rdp-copy-password'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l.sharingRdpCopyPassword),
                  value: _password,
                  onChanged: _busy ? null : (value) => setState(() => _password = value == true),
                )
              else
                Text(l.sharingRdpNoPassword),
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-rdp-copy-confirm'),
          label: l.commonSave,
          onPressed: _busy ? null : _save,
        ),
      ),
    );
  }
}
