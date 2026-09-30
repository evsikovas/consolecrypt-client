import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showSharingSecretPublish(
  BuildContext context, {
  required Credential credential,
  bool passphrase = false,
}) => showAppDialog<void>(
  context,
  secure: true,
  builder: (_) => SharingSecretPublishDialog(credential: credential, passphrase: passphrase),
);

class SharingSecretPublishDialog extends ConsumerStatefulWidget {
  const SharingSecretPublishDialog({required this.credential, this.passphrase = false, super.key});
  final Credential credential;
  final bool passphrase;
  @override
  ConsumerState<SharingSecretPublishDialog> createState() => _SharingSecretPublishDialogState();
}

class _SharingSecretPublishDialogState extends ConsumerState<SharingSecretPublishDialog> {
  Object? _profile;
  String? _metadata;
  List<SharingGrant> _grants = [];
  bool _passphrase = false, _confirmed = false, _busy = false, _failed = false, _ready = false;
  int _generation = 0;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _passphrase =
        widget.passphrase || (widget.credential.secretId == null && widget.credential.passphraseSecretId != null);
    _load();
  }

  Future<void> _load() async {
    final generation = ++_generation;
    setState(() {
      _busy = true;
      _metadata = null;
      _confirmed = false;
      _failed = false;
      _ready = false;
      _grants = [];
    });
    try {
      final service = ref.read(sharingServiceProvider);
      final status = await service.status();
      if (!status.enabled || status.locked || !status.supportsSecrets) throw StateError('unavailable');
      final metadata = await service.previewSecret(widget.credential.id.value, passphrase: _passphrase);
      if (mounted && sharingSessionCurrent(ref, _profile) && generation == _generation) {
        setState(() {
          _metadata = metadata;
          _ready = true;
        });
      }
    } catch (_) {
      if (mounted && generation == _generation) setState(() => _failed = true);
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _publish() async {
    if (_busy || _metadata == null || !_confirmed || _grants.isEmpty || !sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    try {
      final item = await runWithFeedback(
        context,
        () => ref
            .read(sharingServiceProvider)
            .publishSecret(widget.credential.id.value, _grants, passphrase: _passphrase),
      );
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      if (item != null) {
        // The core encrypts directly from the selected credential. No secret
        // value or general-purpose projection JSON is returned to this UI.
        await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
        if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
        refreshSharing(ref);
        closeDialog<void>(context);
      }
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        key: const ValueKey('sharing-secret-publish-dialog'),
        title: l.sharingSecret,
        icon: Icons.key_outlined,
        width: 600,
        content: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.sharingSecretWarning),
                if (_busy) const LinearProgressIndicator(),
                if (_failed) Text(l.sharingUnavailable),
                if (_ready) ...[
                  const SizedBox(height: 12),
                  Text(l.sharingSecretSelection),
                  GlassSelect<bool>(
                    key: const ValueKey('sharing-secret-source'),
                    value: _passphrase,
                    items: [
                      if (widget.credential.secretId != null)
                        GlassSelectItem(value: false, label: l.sharingSecretPrimary),
                      if (widget.credential.passphraseSecretId != null)
                        GlassSelectItem(value: true, label: l.sharingSecretPassphrase),
                    ],
                    onChanged: _busy
                        ? null
                        : (value) {
                            _passphrase = value;
                            _load();
                          },
                  ),
                  const SizedBox(height: 12),
                  _SecretMetadataView(_metadata!),
                  CheckboxListTile(
                    key: const ValueKey('sharing-secret-confirmed'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.sharingSecretConfirmed),
                    value: _confirmed,
                    onChanged: _busy ? null : (value) => setState(() => _confirmed = value == true),
                  ),
                  const SizedBox(height: 16),
                  SharingRecipients(onChanged: (grants) => setState(() => _grants = grants)),
                ],
              ],
            ),
          ),
        ),
        secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-secret-publish'),
          label: l.sharingPublishConfirm,
          onPressed: _busy || _metadata == null || !_confirmed || _grants.isEmpty ? null : _publish,
        ),
      ),
    );
  }
}

/// Deliberately reads only name metadata, even if a malformed backend returns
/// extra JSON fields. Plaintext is never displayed by a generic projection.
class _SecretMetadataView extends StatelessWidget {
  const _SecretMetadataView(this.raw);
  final String raw;
  @override
  Widget build(BuildContext context) {
    final json = jsonDecode(raw) as Map<String, dynamic>;
    final data = json['data'] as Map<String, dynamic>;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(data['name'] as String),
        const Text('••••••••'), // l10n-ignore: masked secret
      ],
    );
  }
}

Future<void> showSharingSecret(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingSecretDialog(item));

class SharingSecretDialog extends ConsumerStatefulWidget {
  const SharingSecretDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingSecretDialog> createState() => _SharingSecretDialogState();
}

class _SharingSecretDialogState extends ConsumerState<SharingSecretDialog> {
  Object? _profile;
  SecretText? _secret;
  Timer? _hide;
  int _generation = 0;
  bool _busy = false, _failed = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
  }

  void _wipe() {
    _generation++;
    _hide?.cancel();
    _hide = null;
    _secret?.wipe();
    _secret = null;
  }

  @override
  void dispose() {
    _wipe();
    super.dispose();
  }

  Future<void> _reveal() async {
    if (_busy ||
        !sharingSessionCurrent(ref, _profile) ||
        widget.item.kind != SharingKind.secret ||
        widget.item.trust != SharingTrust.verified) {
      return;
    }
    _wipe();
    final generation = _generation;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final service = ref.read(sharingServiceProvider);
      final status = await service.status();
      if (!mounted || !sharingSessionCurrent(ref, _profile) || generation != _generation) return;
      if (!status.enabled || status.locked || !status.supportsSecrets) throw StateError('unavailable');
      final secret = await service.revealSecret(widget.item.id);
      if (!mounted || !sharingSessionCurrent(ref, _profile) || generation != _generation) {
        secret.wipe();
        return;
      }
      setState(() => _secret = secret);
      _hide = Timer(const Duration(seconds: 20), () {
        if (mounted) setState(_wipe);
      });
    } catch (_) {
      if (mounted && generation == _generation) setState(() => _failed = true);
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _copy() async {
    if (_busy || _secret == null || !sharingSessionCurrent(ref, _profile)) return;
    _wipe();
    final generation = _generation;
    SecretText? fresh;
    setState(() => _busy = true);
    try {
      // Revalidate access before each copy instead of using the displayed
      // twenty-second buffer, which may predate a membership revocation.
      fresh = await ref.read(sharingServiceProvider).revealSecret(widget.item.id);
      if (!mounted || !sharingSessionCurrent(ref, _profile) || generation != _generation) return;
      await copySecretWithNotice(context, ref, fresh.expose());
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      fresh?.wipe();
      _wipe();
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _importCredential() async {
    if (_busy || !_importable(widget.item) || !sharingSessionCurrent(ref, _profile)) return;
    setState(_wipe);
    final confirmed = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (context) => SharingDialogGuard(
        profile: _profile,
        child: GlassDialog(
          title: context.l10n.sharingCopyPersonal,
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [Text(context.l10n.sharingCopyHelp), Text(context.l10n.sharingSecretWarning)],
          ),
          secondaryActions: [
            GlassButton(label: context.l10n.commonCancel, onPressed: () => closeDialog(context, false)),
          ],
          primaryAction: GlassButton.prominent(
            key: const ValueKey('sharing-secret-import-confirm'),
            label: context.l10n.commonSave,
            onPressed: () => closeDialog(context, true),
          ),
        ),
      ),
    );
    if (confirmed != true || !mounted || !sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    try {
      final status = await ref.read(sharingServiceProvider).status();
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      if (!status.enabled || status.locked || !status.supportsSecrets) throw StateError('unavailable');
      final credential = await runWithFeedback(
        context,
        () => ref.read(sharingServiceProvider).copySecretCredential(widget.item.id),
      );
      if (mounted && credential != null && sharingSessionCurrent(ref, _profile)) {
        showSnack(context, context.l10n.sharingSaved);
      }
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    void isolate() {
      if (!sharingSessionCurrent(ref, _profile)) {
        _wipe();
        if (mounted) setState(() => _busy = false);
      }
    }

    ref.listen(sharingSessionScopeProvider, (_, _) => isolate());
    final l = context.l10n;
    final readable = widget.item.kind == SharingKind.secret && widget.item.trust == SharingTrust.verified;
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        key: const ValueKey('sharing-secret-dialog'),
        title: widget.item.name ?? l.sharingSecret,
        icon: Icons.key_outlined,
        width: 560,
        content: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.sharingSecretWarning),
                Text(l.credentialsRevealHint),
                const SizedBox(height: 12),
                if (_busy) const LinearProgressIndicator(),
                if (_failed) Text(l.sharingUnavailable),
                Text(
                  _secret?.expose() ?? '••••••••', // l10n-ignore: masked secret
                  key: const ValueKey('sharing-secret-value'),
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
                GlassButton(
                  key: const ValueKey('sharing-secret-reveal'),
                  label: _secret == null ? l.credentialsReveal : l.commonHide,
                  onPressed: _busy || !readable
                      ? null
                      : _secret == null
                      ? _reveal
                      : () => setState(_wipe),
                ),
                GlassButton(
                  key: const ValueKey('sharing-secret-copy'),
                  label: l.commonCopy,
                  onPressed: _busy || _secret == null ? null : _copy,
                ),
                if (_importable(widget.item))
                  GlassButton(
                    key: const ValueKey('sharing-secret-import'),
                    label: l.sharingCopyPersonal,
                    onPressed: _busy ? null : _importCredential,
                  ),
                if (widget.item.canEdit && _knownKind(widget.item))
                  GlassButton(
                    key: const ValueKey('sharing-secret-edit'),
                    label: l.sharingEdit,
                    onPressed: _busy
                        ? null
                        : () {
                            setState(_wipe);
                            showSharingSecretEdit(context, widget.item);
                          },
                  ),
              ],
            ),
          ),
        ),
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}

bool _knownKind(SharingItem item) =>
    item.kind == SharingKind.secret &&
    item.trust == SharingTrust.verified &&
    const [
      'password',
      'ssh_private_key',
      'ssh_key_passphrase',
      'api_key',
      'token',
      'other',
    ].contains(item.data?['secret_kind']);

bool _importable(SharingItem item) =>
    _knownKind(item) && const ['password', 'ssh_private_key'].contains(item.data?['secret_kind']);

Future<void> showSharingSecretEdit(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingSecretEditDialog(item));

class SharingSecretEditDialog extends ConsumerStatefulWidget {
  const SharingSecretEditDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingSecretEditDialog> createState() => _SharingSecretEditDialogState();
}

class _SharingSecretEditDialogState extends ConsumerState<SharingSecretEditDialog> {
  final _value = TextEditingController();
  Object? _profile;
  SecretText? _pending;
  bool _confirmed = false, _busy = false, _failed = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
  }

  void _wipe() {
    _value.clear();
    _pending?.wipe();
    _pending = null;
  }

  @override
  void dispose() {
    _wipe();
    _value.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy ||
        !_confirmed ||
        _value.text.isEmpty ||
        !widget.item.canEdit ||
        !_knownKind(widget.item) ||
        !sharingSessionCurrent(ref, _profile)) {
      return;
    }
    final secret = SecretText(_value.text);
    _pending = secret;
    setState(() {
      _busy = true;
      _value.clear();
    });
    try {
      final status = await ref.read(sharingServiceProvider).status();
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      if (!status.enabled || status.locked || !status.supportsSecrets) throw StateError('unavailable');
      final result = await runWithFeedback(
        context,
        () => ref.read(sharingServiceProvider).editSecret(widget.item.id, secret),
      );
      if (result != null && mounted && sharingSessionCurrent(ref, _profile)) {
        await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
        if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
        refreshSharing(ref);
        closeDialog<void>(context);
      }
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      secret.wipe();
      _pending = null;
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    void isolate() {
      if (!sharingSessionCurrent(ref, _profile)) {
        _wipe();
        if (mounted) setState(() => _busy = false);
      }
    }

    ref.listen(sharingSessionScopeProvider, (_, _) => isolate());
    final l = context.l10n;
    final allowed = widget.item.canEdit && _knownKind(widget.item);
    final key = widget.item.data?['secret_kind'] == 'ssh_private_key';
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        key: const ValueKey('sharing-secret-edit-dialog'),
        title: l.sharingEdit,
        width: 560,
        content: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.sharingSecretWarning),
                if (!allowed || _failed) Text(l.sharingUnavailable),
                if (_busy) const LinearProgressIndicator(),
                TextField(
                  key: const ValueKey('sharing-secret-edit-value'),
                  controller: _value,
                  enabled: allowed && !_busy,
                  obscureText: !key,
                  maxLines: key ? 6 : 1,
                  autocorrect: false,
                  enableSuggestions: false,
                  keyboardType: key ? TextInputType.multiline : TextInputType.visiblePassword,
                  decoration: InputDecoration(labelText: l.sharingSecretSelection),
                  onChanged: (_) => setState(() => _confirmed = false),
                ),
                CheckboxListTile(
                  key: const ValueKey('sharing-secret-edit-confirmed'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l.sharingSecretConfirmed),
                  value: _confirmed,
                  onChanged: !allowed || _busy || _value.text.isEmpty
                      ? null
                      : (value) => setState(() => _confirmed = value == true),
                ),
              ],
            ),
          ),
        ),
        secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-secret-edit-save'),
          label: l.commonSave,
          onPressed: !allowed || _busy || !_confirmed || _value.text.isEmpty ? null : _save,
        ),
      ),
    );
  }
}
