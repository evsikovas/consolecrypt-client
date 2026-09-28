import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/credentials/credentials_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum HostAuthMode {
  password(Icons.password_rounded),
  sshKey(Icons.key_rounded),
  agent(Icons.support_agent_rounded),
  inherit(Icons.account_tree_rounded);

  const HostAuthMode(this.icon);

  final IconData icon;

  String localized(AppLocalizations l) => switch (this) {
    HostAuthMode.password => l.hostAuthModePassword,
    HostAuthMode.sshKey => l.hostAuthModeSshKey,
    HostAuthMode.agent => l.hostAuthModeAgent,
    HostAuthMode.inherit => l.hostAuthModeInherit,
  };
}

/// Why [HostAuthDraft.toAuth] cannot produce an auth state yet.
enum HostAuthDraftError {
  keyRequired,
  agentPathRequired;

  String message(AppLocalizations l) => switch (this) {
    HostAuthDraftError.keyRequired => l.hostAuthErrorKeyRequired,
    HostAuthDraftError.agentPathRequired => l.hostAuthErrorAgentPathRequired,
  };
}

bool _isKey(CredentialKind k) =>
    k == CredentialKind.sshPrivateKey || k == CredentialKind.sshCertificate || k == CredentialKind.fido2;

bool _isAgent(CredentialKind k) => k == CredentialKind.osSshAgent || k == CredentialKind.externalAgent;

/// Editable authentication state of the host editor (Termius-style). It
/// never holds a stored secret: an existing inline password is only known
/// to exist ("Password saved ••••••").
final class HostAuthDraft extends ChangeNotifier {
  HostAuthDraft._(this.mode);

  /// Derives the draft from a stored host (or defaults for a new one).
  factory HostAuthDraft.forHost(Host? host, Map<ObjectId, Credential> credentials) {
    if (host == null) return HostAuthDraft._(HostAuthMode.password);
    final id = host.credentialId;
    if (id == null) {
      return host.promptsForPassword
          ? (HostAuthDraft._(HostAuthMode.password)..savePassword = false)
          : HostAuthDraft._(HostAuthMode.inherit);
    }
    final c = credentials[id];
    if (c != null && _isKey(c.kind)) return HostAuthDraft._(HostAuthMode.sshKey)..keyCredentialId = id;
    if (c != null && _isAgent(c.kind)) {
      return HostAuthDraft._(HostAuthMode.agent)
        ..agentKind = c.kind
        ..agentPath.text = c.agentPath ?? '';
    }
    final draft = HostAuthDraft._(HostAuthMode.password);
    if (id == host.inlineCredentialId) {
      draft.hasSavedInline = true;
    } else {
      draft.sharedCredentialId = id;
    }
    return draft;
  }

  HostAuthMode mode;

  /// The user changed something here (stops automatic mode defaults).
  bool touched = false;

  /// Bumped on every change; re-keys form fields whose value is set
  /// programmatically (e.g. a key created via "+ New credential…").
  int epoch = 0;

  // Password mode.
  final TextEditingController password = TextEditingController();
  bool savePassword = true;
  bool hasSavedInline = false;
  bool changingInline = false;

  /// A shared Password credential linked via "Use existing credential…".
  ObjectId? sharedCredentialId;

  // SSH key mode.
  ObjectId? keyCredentialId;
  final TextEditingController keyPassphrase = TextEditingController();
  bool rememberPassphrase = false;

  // Agent mode.
  CredentialKind agentKind = CredentialKind.osSshAgent;
  final TextEditingController agentPath = TextEditingController();

  void update(void Function(HostAuthDraft d) change) {
    change(this);
    touched = true;
    epoch++;
    notifyListeners();
  }

  /// Sets the mode without marking the draft as touched (automatic default).
  void setDefaultMode(HostAuthMode m) {
    if (touched || mode == m) return;
    mode = m;
    notifyListeners();
  }

  /// Links [c] and switches to the matching mode.
  void link(Credential c) => update((d) {
    if (_isKey(c.kind)) {
      d
        ..mode = HostAuthMode.sshKey
        ..keyCredentialId = c.id;
    } else if (_isAgent(c.kind)) {
      d
        ..mode = HostAuthMode.agent
        ..agentKind = c.kind
        ..agentPath.text = c.agentPath ?? '';
    } else {
      d
        ..mode = HostAuthMode.password
        ..sharedCredentialId = c.id;
    }
  });

  bool get _keepsSavedInline => hasSavedInline && !changingInline;

  bool get _savesTypedPassword => savePassword && password.text.isNotEmpty;

  /// The desired auth state for `InventoryService.saveHostWithAuth`, or the
  /// reason it is incomplete.
  (HostAuth?, HostAuthDraftError?) toAuth() => switch (mode) {
    HostAuthMode.inherit => (const HostAuthInherit(), null),
    HostAuthMode.password when sharedCredentialId != null => (HostAuthCredential(sharedCredentialId!), null),
    HostAuthMode.password when _keepsSavedInline => (const HostAuthInlinePassword(), null),
    HostAuthMode.password when _savesTypedPassword => (
      HostAuthInlinePassword(password: SecretText(password.text)),
      null,
    ),
    HostAuthMode.password => (const HostAuthPasswordPrompt(), null),
    HostAuthMode.sshKey when keyCredentialId == null => (null, HostAuthDraftError.keyRequired),
    HostAuthMode.sshKey => (HostAuthCredential(keyCredentialId!), null),
    HostAuthMode.agent when agentKind == CredentialKind.externalAgent && agentPath.text.trim().isEmpty => (
      null,
      HostAuthDraftError.agentPathRequired,
    ),
    HostAuthMode.agent => (HostAuthAgent(kind: agentKind, agentPath: agentPath.text.trim()), null),
  };

  /// A concrete credential id already known (for the effective preview).
  ObjectId? previewCredentialId(Host? stored) => switch (mode) {
    HostAuthMode.password when sharedCredentialId != null => sharedCredentialId,
    HostAuthMode.password when _keepsSavedInline => stored?.inlineCredentialId,
    HostAuthMode.sshKey => keyCredentialId,
    _ => null,
  };

  /// Label for the "Credential" row of the effective preview; `null` for
  /// inherit (resolved by the core).
  String? previewLabel(AppLocalizations l, Map<ObjectId, Credential> credentials) => switch (mode) {
    HostAuthMode.inherit => null,
    HostAuthMode.password when sharedCredentialId != null => switch (credentials[sharedCredentialId]?.name) {
      final name? => l.hostAuthPreviewPasswordShared(name),
      null => l.hostAuthPreviewPasswordDeleted,
    },
    HostAuthMode.password when _keepsSavedInline || _savesTypedPassword => l.hostAuthPreviewPasswordSaved,
    HostAuthMode.password => l.hostAuthPreviewPasswordPrompt,
    HostAuthMode.sshKey =>
      keyCredentialId == null ? '—' : l.hostAuthPreviewSshKey(credentials[keyCredentialId]?.name ?? '?'),
    HostAuthMode.agent =>
      agentKind == CredentialKind.osSshAgent
          ? CredentialKind.osSshAgent.localized(l)
          : l.hostAuthPreviewAgentAt(agentPath.text.trim()),
  };

  @override
  void dispose() {
    password.dispose();
    keyPassphrase.dispose();
    agentPath.dispose();
    super.dispose();
  }
}

/// "Authentication" card of the host editor.
class HostAuthSection extends ConsumerWidget {
  const HostAuthSection({required this.draft, required this.onChanged, super.key, this.groupName});

  final HostAuthDraft draft;

  /// Called after any change (to refresh the effective preview).
  final VoidCallback onChanged;

  /// Name of the host's group, for the inherit explanation.
  final String? groupName;

  void _update(void Function(HostAuthDraft d) change) {
    draft.update(change);
    onChanged();
  }

  Future<void> _useExisting(BuildContext context) async {
    final picked = await showAppDialog<Credential>(context, builder: (_) => const _CredentialPickerDialog());
    if (picked != null) {
      draft.link(picked);
      onChanged();
    }
  }

  Future<void> _createKey(BuildContext context, NewCredentialKind kind) async {
    final created = await showCreateCredentialDialog(context, kind);
    if (created != null) {
      draft.link(created);
      onChanged();
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final credentials = ref.watch(credentialByIdProvider);
    final l10n = context.l10n;
    return ListenableBuilder(
      listenable: draft,
      builder: (context, _) => SectionCard(
        key: const ValueKey('auth-section'),
        title: l10n.hostAuthTitle,
        icon: Icons.lock_person_rounded,
        trailing: GlassButton.plain(
          key: const ValueKey('use-existing-credential'),
          onPressed: () => _useExisting(context),
          icon: Icons.link_rounded,
          label: l10n.hostAuthUseExisting,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GlassSegmented<HostAuthMode>(
              key: const ValueKey('auth-mode'),
              inChrome: false,
              expand: true,
              segments: [
                for (final m in HostAuthMode.values) GlassSegment(value: m, icon: m.icon, label: m.localized(l10n)),
              ],
              selected: draft.mode,
              onChanged: (m) => _update((d) => d.mode = m),
            ),
            const SizedBox(height: GlassSpacing.s12),
            switch (draft.mode) {
              HostAuthMode.password => _passwordMode(context, credentials),
              HostAuthMode.sshKey => _keyMode(context, ref, credentials),
              HostAuthMode.agent => _agentMode(context),
              HostAuthMode.inherit => _inheritMode(context),
            },
          ],
        ),
      ),
    );
  }

  Widget _passwordMode(BuildContext context, Map<ObjectId, Credential> credentials) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final shared = draft.sharedCredentialId;
    if (shared != null) {
      final c = credentials[shared];
      return _LinkedCredential(
        key: const ValueKey('linked-credential'),
        label: c == null
            ? l10n.hostAuthLinkedDeleted
            : l10n.hostAuthLinkedShared(c.name, c.kind.localized(l10n).toLowerCase()),
        onChange: () => _useExisting(context),
        onUnlink: () => _update((d) => d.sharedCredentialId = null),
        unlinkLabel: l10n.hostAuthUnlink,
      );
    }
    if (draft.hasSavedInline && !draft.changingInline) {
      return Row(
        key: const ValueKey('password-saved'),
        children: [
          Icon(Icons.check_circle_rounded, size: 18, color: tokens.palette.success),
          const SizedBox(width: GlassSpacing.s8),
          Text(l10n.hostAuthPasswordSaved),
          Text('••••••', style: tokens.typography.mono.copyWith(color: tokens.palette.label)),
          const Spacer(),
          GlassButton.plain(
            key: const ValueKey('password-change'),
            size: GlassControlSize.sm,
            onPressed: () => _update(
              (d) => d
                ..changingInline = true
                ..savePassword = true,
            ),
            label: l10n.commonChange,
          ),
          GlassButton(
            key: const ValueKey('password-remove'),
            style: GlassButtonStyle.destructiveQuiet,
            size: GlassControlSize.sm,
            onPressed: () => _update(
              (d) => d
                ..changingInline = true
                ..savePassword = false
                ..password.clear(),
            ),
            label: l10n.commonRemove,
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (draft.savePassword)
          SecretField(
            key: const ValueKey('host-password'),
            controller: draft.password,
            label: draft.hasSavedInline ? l10n.hostAuthNewPasswordLabel : l10n.hostAuthPasswordLabel,
            helper: l10n.hostAuthPasswordHelper,
            onChanged: (_) => _update((_) {}),
          ),
        CheckboxListTile(
          key: const ValueKey('save-password'),
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          value: draft.savePassword,
          onChanged: (v) => _update((d) => d.savePassword = v ?? false),
          title: Text(l10n.hostAuthSavePassword),
          subtitle: Text(draft.savePassword ? l10n.hostAuthSavePasswordOn : l10n.hostAuthSavePasswordOff),
        ),
        if (draft.hasSavedInline)
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassButton.plain(
              onPressed: () => _update(
                (d) => d
                  ..changingInline = false
                  ..password.clear()
                  ..savePassword = true,
              ),
              label: l10n.hostAuthKeepSaved,
            ),
          ),
      ],
    );
  }

  Widget _keyMode(BuildContext context, WidgetRef ref, Map<ObjectId, Credential> credentials) {
    final keys = credentials.values.where((c) => _isKey(c.kind)).toList()..sort((a, b) => a.name.compareTo(b.name));
    final selected = credentials[draft.keyCredentialId];
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        DropdownButtonFormField<Object?>(
          key: ValueKey('host-key-credential-${draft.epoch}'),
          isExpanded: true,
          borderRadius: BorderRadius.circular(tokens.radii.menu),
          initialValue: selected?.id,
          decoration: InputDecoration(labelText: l10n.hostAuthKeyLabel),
          items: [
            for (final k in keys)
              DropdownMenuItem(
                value: k.id,
                child: Text('${k.name} · ${k.keyAlgorithm?.localized(l10n) ?? k.kind.localized(l10n)}'),
              ),
            DropdownMenuItem(value: newCredentialEntry, child: Text(l10n.hostsNewCredentialEntry)),
          ],
          onChanged: (v) async {
            if (v == newCredentialEntry) {
              final created = await showNewCredentialChooser(
                context,
                kinds: const [NewCredentialKind.generate, NewCredentialKind.import, NewCredentialKind.certificate],
              );
              if (created != null) {
                draft.link(created);
              } else {
                draft.update((_) {}); // reset the dropdown to the previous key
              }
              onChanged();
            } else if (v is ObjectId) {
              _update((d) => d.keyCredentialId = v);
            }
          },
        ),
        const SizedBox(height: GlassSpacing.s8),
        Wrap(
          spacing: GlassSpacing.s8,
          runSpacing: GlassSpacing.s8,
          children: [
            GlassButton(
              key: const ValueKey('inline-generate-key'),
              onPressed: () => _createKey(context, NewCredentialKind.generate),
              icon: Icons.auto_fix_high_rounded,
              label: l10n.hostAuthGenerateKey,
            ),
            GlassButton(
              key: const ValueKey('inline-import-key'),
              onPressed: () => _createKey(context, NewCredentialKind.import),
              icon: Icons.file_download_rounded,
              label: l10n.hostAuthImportKey,
            ),
          ],
        ),
        if (selected != null) ...[
          const SizedBox(height: GlassSpacing.s8),
          if (selected.fingerprint != null)
            SelectableText(
              selected.fingerprint!,
              style: tokens.typography.mono.copyWith(fontSize: 11, color: tokens.secondaryLabel),
            ),
          if (selected.keyEncrypted && selected.remembersPassphrase)
            Row(
              children: [
                Icon(Icons.check_circle_rounded, size: 18, color: tokens.palette.success),
                const SizedBox(width: GlassSpacing.s8),
                Expanded(child: Text(l10n.hostAuthPassphraseRemembered)),
                GlassButton.plain(
                  size: GlassControlSize.sm,
                  onPressed: () => runWithFeedback(
                    context,
                    () => ref.read(inventoryServiceProvider).forgetKeyPassphrase(selected.id),
                    success: l10n.hostAuthPassphraseForgotten,
                  ),
                  label: l10n.hostAuthForget,
                ),
              ],
            )
          else if (selected.keyEncrypted) ...[
            const SizedBox(height: GlassSpacing.s8),
            SecretField(
              key: const ValueKey('key-passphrase'),
              controller: draft.keyPassphrase,
              label: l10n.hostAuthKeyPassphraseLabel,
              helper: l10n.hostAuthKeyPassphraseHelper,
              onChanged: (_) => _update((_) {}),
            ),
            CheckboxListTile(
              key: const ValueKey('remember-key-passphrase'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: draft.rememberPassphrase,
              onChanged: (v) => _update((d) => d.rememberPassphrase = v ?? false),
              title: Text(l10n.hostAuthRememberPassphrase),
              subtitle: Text(l10n.hostAuthRememberPassphraseHelper),
            ),
          ],
        ],
      ],
    );
  }

  Widget _agentMode(BuildContext context) {
    final l10n = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GlassSegmented<CredentialKind>(
          key: const ValueKey('agent-kind'),
          inChrome: false,
          expand: true,
          segments: [
            for (final k in const [CredentialKind.osSshAgent, CredentialKind.externalAgent])
              GlassSegment(value: k, label: k.localized(l10n)),
          ],
          selected: draft.agentKind,
          onChanged: (k) => _update((d) => d.agentKind = k),
        ),
        if (draft.agentKind == CredentialKind.externalAgent) ...[
          const SizedBox(height: GlassSpacing.s12),
          TextField(
            key: const ValueKey('agent-path'),
            controller: draft.agentPath,
            decoration: InputDecoration(
              labelText: l10n.hostAuthAgentPathLabel,
              hintText: l10n.hostAuthAgentPathHint(
                '~/.1password/agent.sock',
                r'\\.\pipe\openssh-ssh-agent', // l10n-ignore: example Windows pipe path
              ),
            ),
            onChanged: (_) => _update((_) {}),
          ),
        ],
        const SizedBox(height: GlassSpacing.s8),
        Text(l10n.hostAuthAgentNote),
      ],
    );
  }

  Widget _inheritMode(BuildContext context) {
    final group = groupName;
    return group == null
        ? InfoBanner(tone: BannerTone.warning, message: context.l10n.hostAuthInheritNoGroup)
        : Text(context.l10n.hostAuthInheritFromGroup(group));
  }
}

class _LinkedCredential extends StatelessWidget {
  const _LinkedCredential({
    required this.label,
    required this.onChange,
    required this.onUnlink,
    required this.unlinkLabel,
    super.key,
  });

  final String label;
  final VoidCallback onChange;
  final VoidCallback onUnlink;
  final String unlinkLabel;

  @override
  Widget build(BuildContext context) => Row(
    children: [
      Icon(Icons.link_rounded, size: 18, color: GlassTokens.of(context).palette.accent),
      const SizedBox(width: GlassSpacing.s8),
      Expanded(child: Text(label)),
      GlassButton.plain(size: GlassControlSize.sm, onPressed: onChange, label: context.l10n.hostAuthChangeLinked),
      GlassButton.plain(size: GlassControlSize.sm, onPressed: onUnlink, label: unlinkLabel),
    ],
  );
}

/// "Use existing credential…": all credentials (shared across hosts) plus
/// "+ New credential…".
class _CredentialPickerDialog extends ConsumerWidget {
  const _CredentialPickerDialog();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final credentials = ref.watch(credentialsProvider).value ?? const <Credential>[];
    final hosts = ref.watch(hostsProvider).value ?? const <Host>[];
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('credential-picker'),
      title: l10n.hostAuthPickerTitle,
      width: 520,
      content: SizedBox(
        height: 380,
        child: credentials.isEmpty
            ? EmptyState(icon: Icons.key_rounded, title: l10n.hostAuthPickerEmpty, compact: true)
            : ListView(
                children: [
                  for (final c in credentials)
                    ListTile(
                      key: ValueKey('pick-credential-${c.name}'),
                      leading: Icon(credentialIcon(c.kind)),
                      title: Text(c.name, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
                      subtitle: Text(
                        [
                          c.kind.localized(l10n),
                          if (c.username case final username?) l10n.hostAuthPickerUser(username),
                          l10n.hostAuthPickerUsedBy(hosts.where((h) => h.credentialId == c.id).length),
                        ].join(' · '),
                      ),
                      onTap: () => closeDialog(context, c),
                    ),
                ],
              ),
      ),
      leadingAction: GlassButton.plain(
        key: const ValueKey('picker-new-credential'),
        onPressed: () async {
          final created = await showNewCredentialChooser(context);
          if (created != null && context.mounted) closeDialog(context, created);
        },
        icon: Icons.add_rounded,
        label: l10n.hostAuthPickerNew,
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Credential>(context))],
    );
  }
}
