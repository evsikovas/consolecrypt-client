import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/obscurable_text_controller.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shared chrome for the credential dialogs: a [GlassDialog] (the route's
/// material — secure for anything that takes a secret, see
/// [showCreateCredentialDialog]).
class _CredentialDialogFrame extends StatelessWidget {
  const _CredentialDialogFrame({
    required this.title,
    required this.children,
    required this.onSave,
    required this.busy,
    this.saveLabel,
    this.error,
  });

  final String title;
  final List<Widget> children;
  final VoidCallback? onSave;
  final bool busy;

  /// Confirm button label; defaults to "Save".
  final String? saveLabel;
  final String? error;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return GlassDialog(
      title: title,
      width: 560,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ...children,
            if (error != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              GateErrorText(key: const ValueKey('credential-error'), text: error!),
            ],
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Credential>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('credential-save'),
        busy: busy,
        onPressed: busy ? null : onSave,
        label: saveLabel ?? l10n.commonSave,
      ),
    );
  }
}

/// Dialog-local failures that are not [AppException]s.
enum _DialogError { passphraseMismatch }

mixin _Busy<T extends ConsumerStatefulWidget> on ConsumerState<T> {
  bool busy = false;

  /// The last failure: an [AppException] or a [_DialogError].
  Object? error;

  /// [error] as user-facing text (`null` when there is none).
  String? errorText(AppLocalizations l10n) => switch (error) {
    null => null,
    _DialogError.passphraseMismatch => l10n.credentialGeneratePassphraseMismatch,
    final Object e => errorMessage(l10n, e),
  };

  Future<void> guard(Future<void> Function() action) async {
    setState(() {
      busy = true;
      error = null;
    });
    try {
      await action();
    } on AppException catch (e) {
      if (mounted) setState(() => error = e);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }
}

class PasswordCredentialDialog extends ConsumerStatefulWidget {
  const PasswordCredentialDialog({super.key});

  @override
  ConsumerState<PasswordCredentialDialog> createState() => _PasswordCredentialDialogState();
}

class _PasswordCredentialDialogState extends ConsumerState<PasswordCredentialDialog> with _Busy {
  final _name = TextEditingController();
  final _username = TextEditingController();
  final _password = TextEditingController();

  @override
  void dispose() {
    _name.dispose();
    _username.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _save() => guard(() async {
    final secret = SecretText(_password.text);
    try {
      final created = await ref
          .read(inventoryServiceProvider)
          .createPasswordCredential(name: _name.text, username: _username.text, password: secret);
      _password.clear();
      if (mounted) closeDialog(context, created);
    } finally {
      secret.wipe();
    }
  });

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return _CredentialDialogFrame(
      title: l10n.credentialDialogNewPasswordTitle,
      busy: busy,
      error: errorText(l10n),
      onSave: _save,
      children: [
        TextField(
          controller: _name,
          autofocus: true,
          decoration: InputDecoration(labelText: l10n.commonName),
        ),
        const SizedBox(height: GlassSpacing.s12),
        TextField(
          controller: _username,
          decoration: InputDecoration(labelText: l10n.credentialDialogUsernameOptional),
        ),
        const SizedBox(height: GlassSpacing.s12),
        SecretField(controller: _password, label: l10n.credentialDialogPasswordLabel),
      ],
    );
  }
}

class GenerateKeyDialog extends ConsumerStatefulWidget {
  const GenerateKeyDialog({super.key});

  @override
  ConsumerState<GenerateKeyDialog> createState() => _GenerateKeyDialogState();
}

class _GenerateKeyDialogState extends ConsumerState<GenerateKeyDialog> with _Busy {
  final _name = TextEditingController();
  final _comment = TextEditingController();
  final _passphrase = TextEditingController();
  final _confirm = TextEditingController();
  KeyAlgorithm _algorithm = KeyAlgorithm.ed25519;
  bool _remember = false;
  Credential? _created;

  @override
  void dispose() {
    for (final c in [_name, _comment, _passphrase, _confirm]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _generate() async {
    if (_passphrase.text != _confirm.text) {
      setState(() => error = _DialogError.passphraseMismatch);
      return;
    }
    await guard(() async {
      final pass = _passphrase.text.isEmpty ? null : SecretText(_passphrase.text);
      try {
        final created = await ref
            .read(inventoryServiceProvider)
            .generateKeyCredential(
              name: _name.text,
              algorithm: _algorithm,
              comment: _comment.text,
              passphrase: pass,
              rememberPassphrase: _remember,
            );
        _passphrase.clear();
        _confirm.clear();
        if (mounted) setState(() => _created = created);
      } finally {
        pass?.wipe();
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final created = _created;
    if (created != null) {
      final tokens = GlassTokens.of(context);
      final mono = tokens.typography.mono.copyWith(fontSize: 11, color: tokens.palette.label);
      return GlassDialog(
        icon: Icons.check_circle_rounded,
        iconTone: GlassTone.success,
        title: l10n.credentialGenerateDoneTitle,
        width: 560,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l10n.credentialGenerateDoneMessage),
            const SizedBox(height: GlassSpacing.s8),
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              padding: const EdgeInsets.all(GlassSpacing.s12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SelectableText(created.publicKey ?? '', key: const ValueKey('generated-public-key'), style: mono),
                  const SizedBox(height: GlassSpacing.s8),
                  SelectableText(created.fingerprint ?? '', style: mono.copyWith(color: tokens.secondaryLabel)),
                ],
              ),
            ),
          ],
        ),
        leadingAction: GlassButton(
          onPressed: () => copyPlainWithNotice(context, ref, created.publicKey ?? '', what: l10n.copyWhatPublicKey),
          icon: Icons.copy_rounded,
          label: l10n.credentialsCopyPublicKey,
        ),
        primaryAction: GlassButton.prominent(
          key: const ValueKey('generated-done'),
          onPressed: () => closeDialog(context, created),
          label: l10n.commonDone,
        ),
      );
    }
    return _CredentialDialogFrame(
      title: l10n.credentialGenerateTitle,
      busy: busy,
      error: errorText(l10n),
      onSave: _generate,
      saveLabel: l10n.credentialGenerateAction,
      children: [
        TextField(
          controller: _name,
          autofocus: true,
          decoration: InputDecoration(labelText: l10n.commonName),
        ),
        const SizedBox(height: GlassSpacing.s12),
        GlassSegmented<KeyAlgorithm>(
          key: const ValueKey('key-algorithm'),
          inChrome: false,
          expand: true,
          segments: [
            for (final a in KeyAlgorithm.generatable)
              GlassSegment(
                value: a,
                label: a == KeyAlgorithm.ed25519
                    ? l10n.credentialGenerateRecommended(a.localized(l10n))
                    : a.localized(l10n),
              ),
          ],
          selected: _algorithm,
          onChanged: (a) => setState(() => _algorithm = a),
        ),
        if (_algorithm != KeyAlgorithm.ed25519)
          Padding(
            padding: const EdgeInsets.only(top: GlassSpacing.s6),
            child: Text(l10n.credentialGenerateRsaNotice),
          ),
        const SizedBox(height: GlassSpacing.s12),
        TextField(
          controller: _comment,
          // The hint is an example `user@host` comment (not translated).
          decoration: InputDecoration(
            labelText: l10n.credentialGenerateCommentLabel,
            hintText: 'you@laptop', // l10n-ignore: key comment example
          ),
        ),
        const SizedBox(height: GlassSpacing.s12),
        SecretField(
          controller: _passphrase,
          label: l10n.credentialGeneratePassphraseLabel,
          helper: l10n.credentialGeneratePassphraseHelper,
          onChanged: (_) => setState(() {}),
        ),
        if (_passphrase.text.isNotEmpty) ...[
          const SizedBox(height: GlassSpacing.s12),
          SecretField(controller: _confirm, label: l10n.credentialGenerateRepeatPassphrase),
          CheckboxListTile(
            contentPadding: EdgeInsets.zero,
            value: _remember,
            onChanged: (v) => setState(() => _remember = v ?? false),
            title: Text(l10n.credentialDialogRememberPassphrase),
            subtitle: Text(l10n.credentialGenerateRememberSubtitle),
          ),
        ],
      ],
    );
  }
}

/// Import an OpenSSH / PEM private key (optionally with a certificate). An
/// encrypted key keeps its passphrase protection (CLIENT_SPEC §7.6).
class ImportKeyDialog extends ConsumerStatefulWidget {
  const ImportKeyDialog({super.key, this.withCertificate = false});

  final bool withCertificate;

  @override
  ConsumerState<ImportKeyDialog> createState() => _ImportKeyDialogState();
}

class _ImportKeyDialogState extends ConsumerState<ImportKeyDialog> with _Busy {
  final _name = TextEditingController();
  final _username = TextEditingController();
  final _key = ObscurableTextController();
  final _passphrase = TextEditingController();
  final _certificate = TextEditingController();
  bool _remember = false;
  KeyInspection? _inspection;
  Timer? _debounce;

  @override
  void dispose() {
    _debounce?.cancel();
    for (final c in [_name, _username, _key, _passphrase, _certificate]) {
      c.dispose();
    }
    super.dispose();
  }

  void _onKeyChanged(String _) {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 250), () async {
      final secret = SecretText(_key.text);
      try {
        final result = await ref.read(inventoryServiceProvider).inspectPrivateKey(secret);
        if (mounted) setState(() => _inspection = result);
      } on AppException catch (e) {
        if (mounted) setState(() => error = e);
      } finally {
        secret.wipe();
      }
    });
  }

  Future<void> _import() => guard(() async {
    final key = SecretText(_key.text);
    final pass = _passphrase.text.isEmpty ? null : SecretText(_passphrase.text);
    try {
      final created = await ref
          .read(inventoryServiceProvider)
          .importKeyCredential(
            name: _name.text,
            username: _username.text,
            privateKey: key,
            passphrase: pass,
            rememberPassphrase: _remember,
            certificate: widget.withCertificate ? _certificate.text : null,
          );
      _key.clear();
      _passphrase.clear();
      if (mounted) closeDialog(context, created);
    } finally {
      key.wipe();
      pass?.wipe();
    }
  });

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final inspection = _inspection;
    final tokens = GlassTokens.of(context);
    final mono = tokens.typography.mono.copyWith(fontSize: 11, color: tokens.palette.label);
    return _CredentialDialogFrame(
      title: widget.withCertificate ? l10n.credentialImportTitleWithCertificate : l10n.credentialImportTitle,
      busy: busy,
      error: errorText(l10n),
      onSave: inspection?.valid == true ? _import : null,
      saveLabel: l10n.credentialImportAction,
      children: [
        TextField(
          controller: _name,
          autofocus: true,
          decoration: InputDecoration(labelText: l10n.commonName),
        ),
        const SizedBox(height: GlassSpacing.s12),
        TextField(
          controller: _username,
          decoration: InputDecoration(labelText: l10n.credentialDialogUsernameOptional),
        ),
        const SizedBox(height: GlassSpacing.s12),
        Row(
          children: [
            Text(
              l10n.credentialImportPrivateKey,
              style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
            ),
            const Spacer(),
            GlassButton.plain(
              size: GlassControlSize.sm,
              onPressed: () => setState(() => _key.obscured = !_key.obscured),
              icon: _key.obscured ? Icons.visibility_rounded : Icons.visibility_off_rounded,
              label: _key.obscured ? l10n.commonShow : l10n.commonHide,
            ),
            // TODO(client/ui): "Choose file…" for private keys — the desktop FileDialogService
            // lands at M4 and app-core must read the file itself (no temp copies in Dart);
            // next: add InventoryService.importKeyFile(path) and wire this button.
          ],
        ),
        // The key is drawn as bullets until "Show" — never blurred (a blur still
        // renders the secret; LIQUID_GLASS_SPEC §4.14).
        TextField(
          key: const ValueKey('private-key-input'),
          controller: _key,
          minLines: 4,
          maxLines: 8,
          autocorrect: false,
          enableSuggestions: false,
          enableIMEPersonalizedLearning: false,
          style: mono,
          decoration: const InputDecoration(
            hintText: '-----BEGIN OPENSSH PRIVATE KEY-----', // l10n-ignore: key format example
          ),
          onChanged: _onKeyChanged,
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (inspection != null)
          inspection.valid
              ? InfoBanner(
                  tone: BannerTone.success,
                  message: [
                    [
                      inspection.algorithm?.localized(l10n) ?? l10n.credentialImportUnknownAlgorithm,
                      inspection.fingerprint ?? '',
                    ].join(' · '),
                    if (inspection.encrypted) l10n.credentialImportEncryptedNotice,
                  ].join('\n'),
                )
              // `inspection.error` is an English service diagnostic: shown only as detail
              // under the localized title.
              : InfoBanner(
                  tone: BannerTone.danger,
                  title: inspection.error == null ? null : l10n.credentialImportInvalidKey,
                  message: inspection.error ?? l10n.credentialImportInvalidKey,
                ),
        if (inspection?.encrypted ?? false) ...[
          CheckboxListTile(
            key: const ValueKey('remember-passphrase'),
            contentPadding: EdgeInsets.zero,
            value: _remember,
            onChanged: (v) => setState(() => _remember = v ?? false),
            title: Text(l10n.credentialDialogRememberPassphrase),
            subtitle: Text(l10n.credentialImportRememberSubtitle),
          ),
          if (_remember) SecretField(controller: _passphrase, label: l10n.credentialDialogKeyPassphrase),
        ],
        if (widget.withCertificate) ...[
          const SizedBox(height: GlassSpacing.s12),
          TextField(
            controller: _certificate,
            minLines: 2,
            maxLines: 4,
            style: mono,
            decoration: InputDecoration(
              labelText: l10n.credentialImportCertificateLabel,
              hintText: 'ssh-ed25519-cert-v01@openssh.com AAAA…', // l10n-ignore: certificate format example
            ),
          ),
        ],
      ],
    );
  }
}

class AgentCredentialDialog extends ConsumerStatefulWidget {
  const AgentCredentialDialog({super.key});

  @override
  ConsumerState<AgentCredentialDialog> createState() => _AgentCredentialDialogState();
}

class _AgentCredentialDialogState extends ConsumerState<AgentCredentialDialog> with _Busy {
  final _name = TextEditingController();
  final _username = TextEditingController();
  final _path = TextEditingController();
  CredentialKind _kind = CredentialKind.osSshAgent;

  /// The localized default name last put into [_name]; it follows language
  /// changes unless the user edited it.
  String? _defaultName;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final localized = context.l10n.credentialDialogAgentDefaultName;
    if (_defaultName == null || _name.text == _defaultName) _name.text = localized;
    _defaultName = localized;
  }

  @override
  void dispose() {
    _name.dispose();
    _username.dispose();
    _path.dispose();
    super.dispose();
  }

  Future<void> _save() => guard(() async {
    final created = await ref
        .read(inventoryServiceProvider)
        .createAgentCredential(name: _name.text, kind: _kind, username: _username.text, agentPath: _path.text);
    if (mounted) closeDialog(context, created);
  });

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return _CredentialDialogFrame(
      title: l10n.credentialDialogAgentTitle,
      busy: busy,
      error: errorText(l10n),
      onSave: _save,
      children: [
        GlassSegmented<CredentialKind>(
          key: const ValueKey('agent-credential-kind'),
          inChrome: false,
          expand: true,
          segments: [
            GlassSegment(value: CredentialKind.osSshAgent, label: CredentialKind.osSshAgent.localized(l10n)),
            GlassSegment(value: CredentialKind.externalAgent, label: CredentialKind.externalAgent.localized(l10n)),
          ],
          selected: _kind,
          onChanged: (k) => setState(() => _kind = k),
        ),
        const SizedBox(height: GlassSpacing.s12),
        TextField(
          controller: _name,
          decoration: InputDecoration(labelText: l10n.commonName),
        ),
        const SizedBox(height: GlassSpacing.s12),
        TextField(
          controller: _username,
          decoration: InputDecoration(labelText: l10n.credentialDialogUsernameOptional),
        ),
        if (_kind == CredentialKind.externalAgent) ...[
          const SizedBox(height: GlassSpacing.s12),
          TextField(
            controller: _path,
            decoration: InputDecoration(
              labelText: l10n.credentialDialogAgentPathLabel,
              hintText: l10n.credentialDialogAgentPathHint(
                '~/.1password/agent.sock',
                r'\\.\pipe\openssh-ssh-agent', // l10n-ignore: pipe path example
              ),
            ),
          ),
        ],
        const SizedBox(height: GlassSpacing.s8),
        Text(l10n.credentialDialogAgentNotice),
      ],
    );
  }
}

/// The credential creation flows (Credentials screen, host editor, pickers).
enum NewCredentialKind {
  password(Icons.password_rounded),
  generate(Icons.auto_fix_high_rounded),
  import(Icons.file_download_rounded),
  certificate(Icons.workspace_premium_rounded),
  agent(Icons.support_agent_rounded);

  const NewCredentialKind(this.icon);

  final IconData icon;

  /// Menu / chooser entry text.
  String localizedLabel(AppLocalizations l10n) => switch (this) {
    NewCredentialKind.password => l10n.credentialsNewPassword,
    NewCredentialKind.generate => l10n.credentialsNewGenerateKey,
    NewCredentialKind.import => l10n.credentialsNewImportKey,
    NewCredentialKind.certificate => l10n.credentialsNewCertificate,
    NewCredentialKind.agent => l10n.credentialsNewAgent,
  };
}

/// Opens the dialog for [kind]; returns the created credential (or `null`).
/// Anything that takes a secret (password, key, passphrase) opens on the
/// secure material (§4.4, §4.7): opaque, no blur.
Future<Credential?> showCreateCredentialDialog(BuildContext context, NewCredentialKind kind) =>
    showAppDialog<Credential>(
      context,
      secure: kind != NewCredentialKind.agent,
      builder: (_) => switch (kind) {
        NewCredentialKind.password => const PasswordCredentialDialog(),
        NewCredentialKind.generate => const GenerateKeyDialog(),
        NewCredentialKind.import => const ImportKeyDialog(),
        NewCredentialKind.certificate => const ImportKeyDialog(withCertificate: true),
        NewCredentialKind.agent => const AgentCredentialDialog(),
      },
    );

/// "+ New credential…": choose a kind, then create it.
Future<Credential?> showNewCredentialChooser(BuildContext context, {List<NewCredentialKind>? kinds}) async {
  final kind = await showAppDialog<NewCredentialKind>(
    context,
    builder: (context) {
      final l10n = context.l10n;
      return GlassDialog(
        key: const ValueKey('new-credential-chooser'),
        title: l10n.credentialsNew,
        width: 420,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final k in kinds ?? NewCredentialKind.values)
              ListTile(
                key: ValueKey('new-credential-${k.name}'),
                leading: Icon(k.icon),
                title: Text(k.localizedLabel(l10n)),
                trailing: const Icon(Icons.chevron_right_rounded),
                onTap: () => closeDialog(context, k),
              ),
          ],
        ),
        secondaryActions: [
          GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<NewCredentialKind>(context)),
        ],
      );
    },
  );
  if (kind == null || !context.mounted) return null;
  return showCreateCredentialDialog(context, kind);
}

/// Sentinel dropdown value for a "+ New credential…" entry.
const Object newCredentialEntry = _NewCredentialEntry();

final class _NewCredentialEntry {
  const _NewCredentialEntry();
}
