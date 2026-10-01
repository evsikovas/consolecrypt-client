import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_secrets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

IconData credentialIcon(CredentialKind kind) => switch (kind) {
  CredentialKind.password => Icons.password_rounded,
  CredentialKind.sshPrivateKey => Icons.key_rounded,
  CredentialKind.sshCertificate => Icons.workspace_premium_rounded,
  CredentialKind.osSshAgent => Icons.support_agent_rounded,
  CredentialKind.externalAgent => Icons.cable_rounded,
  CredentialKind.fido2 => Icons.usb_rounded,
};

class CredentialsScreen extends ConsumerWidget {
  const CredentialsScreen({super.key});

  Future<void> _create(BuildContext context, NewCredentialKind kind) => showCreateCredentialDialog(context, kind);

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final credsAsync = ref.watch(credentialsProvider);
    final hosts = ref.watch(hostsProvider).value ?? const <Host>[];
    final groups = ref.watch(groupsProvider).value ?? const <Group>[];
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    return PageScaffold(
      title: l10n.credentialsTitle,
      subtitle: l10n.credentialsSubtitle,
      actions: [
        GlassMenuButton<NewCredentialKind>(
          entries: [
            for (final k in NewCredentialKind.values)
              GlassMenuItem(
                key: ValueKey(switch (k) {
                  NewCredentialKind.password => 'new-password',
                  NewCredentialKind.generate => 'new-generate',
                  NewCredentialKind.import => 'new-import',
                  NewCredentialKind.certificate => 'new-certificate',
                  NewCredentialKind.agent => 'new-agent',
                }),
                value: k,
                label: k.localizedLabel(l10n),
                icon: k.icon,
              ),
          ],
          onSelected: (k) => _create(context, k),
          builder: (context, open) => GlassButton(
            key: const ValueKey('add-credential'),
            onPressed: open,
            icon: Icons.add_rounded,
            label: l10n.credentialsNew,
          ),
        ),
      ],
      body: AsyncValueView(
        value: credsAsync,
        data: (creds) => creds.isEmpty
            ? EmptyState(
                icon: Icons.key_rounded,
                title: l10n.credentialsEmptyTitle,
                message: l10n.credentialsEmptyMessage,
              )
            : ContentList(
                itemCount: creds.length,
                itemBuilder: (context, i) {
                  final c = creds[i];
                  final usedBy =
                      hosts.where((h) => h.credentialId == c.id).length +
                      groups.where((g) => g.inheritedCredentialId == c.id).length;
                  return ListTile(
                    key: ValueKey('credential-${c.name}'),
                    leading: SizedBox.square(
                      dimension: 32,
                      child: DecoratedBox(
                        decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: const CircleBorder()),
                        child: Icon(credentialIcon(c.kind), size: 18, color: tokens.secondaryLabel),
                      ),
                    ),
                    title: Text(c.name, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
                    subtitle: Text(
                      [
                        c.kind.localized(l10n),
                        if (c.keyAlgorithm != null) c.keyAlgorithm!.localized(l10n),
                        if (c.username != null) l10n.credentialsListUser(c.username!),
                        if (c.keyEncrypted)
                          c.remembersPassphrase
                              ? l10n.credentialsListPassphraseRemembered
                              : l10n.credentialsListPassphraseProtected,
                        if (usedBy > 0) l10n.credentialsListUsedBy(usedBy),
                      ].join(' · '),
                    ),
                    trailing: c.fingerprint == null
                        ? null
                        : Text(
                            '${c.fingerprint!.substring(0, c.fingerprint!.length.clamp(0, 20))}…',
                            style: t.mono.copyWith(fontSize: 11, color: tokens.secondaryLabel),
                          ),
                    onTap: () => showAppDialog<void>(
                      context,
                      // Secret reveal: the secure material (§4.7).
                      secure: true,
                      builder: (_) => CredentialDetailsDialog(credential: c),
                    ),
                  );
                },
              ),
      ),
    );
  }
}

/// Details with explicit reveal/copy of secrets and public key copy.
class CredentialDetailsDialog extends ConsumerStatefulWidget {
  const CredentialDetailsDialog({required this.credential, super.key});

  final Credential credential;

  @override
  ConsumerState<CredentialDetailsDialog> createState() => _CredentialDetailsDialogState();
}

class _CredentialDetailsDialogState extends ConsumerState<CredentialDetailsDialog> {
  String? _revealed;
  Timer? _hideTimer;
  Object? _profile;
  Object? _vault;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _profile = ref.read(activeProfileProvider)?.id;
    _vault = ref.read(vaultStatusProvider).value?.vaultId;
  }

  bool get _currentSession =>
      mounted &&
      _profile != null &&
      ref.read(activeProfileProvider)?.id == _profile &&
      ref.read(vaultStatusProvider).value?.vaultId == _vault &&
      ref.read(vaultStatusProvider).value?.isUnlocked == true;

  void _forget() {
    _generation++;
    _hideTimer?.cancel();
    _hideTimer = null;
    _revealed = null;
  }

  @override
  void dispose() {
    _forget();
    super.dispose();
  }

  Future<SecretText?> _fetch() async {
    if (!_currentSession) return null;
    final generation = _generation;
    final secret = await runWithFeedback(
      context,
      () => ref.read(inventoryServiceProvider).revealCredentialSecret(widget.credential.id),
    );
    if (!_currentSession || generation != _generation) {
      secret?.wipe();
      return null;
    }
    return secret;
  }

  Future<void> _reveal() async {
    final secret = await _fetch();
    if (secret == null) return;
    try {
      setState(() => _revealed = secret.expose());
    } finally {
      secret.wipe();
    }
    _hideTimer?.cancel();
    _hideTimer = Timer(const Duration(seconds: 20), () {
      if (mounted) setState(() => _revealed = null);
    });
  }

  Future<void> _copy() async {
    final secret = await _fetch();
    if (secret == null) return;
    try {
      if (!mounted) return;
      await copySecretWithNotice(context, ref, secret.expose(), what: context.l10n.copyWhatPassword);
    } finally {
      secret.wipe();
    }
  }

  Future<void> _delete() async {
    final l10n = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l10n.credentialsDeleteTitle(widget.credential.name),
      message: l10n.credentialsDeleteMessage,
      confirmLabel: l10n.commonDelete,
      destructive: true,
    );
    if (!ok || !mounted) return;
    await runWithFeedback(context, () => ref.read(inventoryServiceProvider).deleteCredential(widget.credential.id));
    if (mounted) closeDialog<void>(context);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(activeProfileProvider.select((p) => p?.id), (before, after) {
      if (before != after) setState(_forget);
    });
    ref.listen(vaultStatusProvider.select((s) => s.value?.isUnlocked ?? false), (_, unlocked) {
      if (!unlocked) setState(_forget);
    });
    final l10n = context.l10n;
    final c = widget.credential;
    final isPassword = c.kind == CredentialKind.password;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final mono = t.mono.copyWith(fontSize: 11, color: tokens.palette.label);
    return GlassDialog(
      key: const ValueKey('credential-details'),
      icon: credentialIcon(c.kind),
      title: c.name,
      width: 560,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            LabeledValue(label: l10n.credentialsDetailsType, value: Text(c.kind.localized(l10n))),
            if (c.username != null) LabeledValue(label: l10n.credentialsDetailsUsername, value: Text(c.username!)),
            if (c.keyAlgorithm != null)
              LabeledValue(label: l10n.credentialsDetailsAlgorithm, value: Text(c.keyAlgorithm!.localized(l10n))),
            if (c.fingerprint != null)
              LabeledValue(
                label: l10n.credentialsDetailsFingerprint,
                value: SelectableText(c.fingerprint!, style: mono.copyWith(fontSize: 12)),
              ),
            if (c.keyEncrypted)
              LabeledValue(
                label: l10n.credentialDialogKeyPassphrase,
                value: Text(
                  c.remembersPassphrase
                      ? l10n.credentialsDetailsPassphraseRemembered
                      : l10n.credentialsDetailsPassphraseAsked,
                ),
              ),
            if (c.agentPath != null)
              LabeledValue(label: l10n.credentialsDetailsAgentSocket, value: SelectableText(c.agentPath!)),
            LabeledValue(label: l10n.credentialsDetailsCreated, value: Text(formatDate(l10n, c.createdAt))),
            if (c.publicKey != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              Text(l10n.credentialsDetailsPublicKey, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
              const SizedBox(height: GlassSpacing.s4),
              ContentSurface(
                kind: ContentSurfaceKind.inset,
                padding: const EdgeInsets.all(GlassSpacing.s8),
                child: SelectableText(c.publicKey!, style: mono),
              ),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: GlassButton.plain(
                  onPressed: () => copyPlainWithNotice(context, ref, c.publicKey!, what: l10n.copyWhatPublicKey),
                  icon: Icons.copy_rounded,
                  label: l10n.credentialsCopyPublicKey,
                ),
              ),
            ],
            if (c.certificate != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              Text(l10n.credentialsDetailsCertificate, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
              const SizedBox(height: GlassSpacing.s4),
              ContentSurface(
                kind: ContentSurfaceKind.inset,
                padding: const EdgeInsets.all(GlassSpacing.s8),
                child: SelectableText(c.certificate!, style: mono),
              ),
            ],
            if (ref.watch(activeProfileProvider)?.isSynced == true) ...[
              if (c.secretId != null)
                GlassButton(
                  label: l10n.sharingPublish,
                  icon: Icons.share_outlined,
                  onPressed: () => showSharingSecretPublish(context, credential: c),
                ),
              if (c.passphraseSecretId != null)
                GlassButton(
                  label: l10n.sharingSecretPassphrase,
                  icon: Icons.share_outlined,
                  onPressed: () => showSharingSecretPublish(context, credential: c, passphrase: true),
                ),
            ],
            if (isPassword) ...[
              const SizedBox(height: GlassSpacing.s12),
              ContentSurface(
                kind: ContentSurfaceKind.inset,
                padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s6),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        _revealed ?? '••••••••••••',
                        key: const ValueKey('password-value'),
                        style: t.mono.copyWith(color: tokens.palette.label),
                      ),
                    ),
                    GlassButton.plain(
                      key: const ValueKey('reveal-password'),
                      size: GlassControlSize.sm,
                      icon: _revealed == null ? Icons.visibility_rounded : Icons.visibility_off_rounded,
                      onPressed: _revealed == null ? _reveal : () => setState(() => _revealed = null),
                      label: _revealed == null ? l10n.credentialsReveal : l10n.commonHide,
                    ),
                    GlassButton.plain(
                      size: GlassControlSize.sm,
                      icon: Icons.copy_rounded,
                      onPressed: _copy,
                      label: l10n.commonCopy,
                    ),
                  ],
                ),
              ),
              const SizedBox(height: GlassSpacing.s4),
              Text(l10n.credentialsRevealHint, style: t.callout.copyWith(color: tokens.secondaryLabel)),
            ],
          ],
        ),
      ),
      leadingAction: GlassButton(
        key: const ValueKey('credential-delete'),
        style: GlassButtonStyle.destructiveQuiet,
        icon: Icons.delete_rounded,
        onPressed: _delete,
        label: l10n.commonDelete,
      ),
      primaryAction: GlassButton.prominent(
        autofocus: true,
        onPressed: () => closeDialog<void>(context),
        label: l10n.commonClose,
      ),
    );
  }
}
