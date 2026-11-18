import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A selected Native frame is source metadata, never a password or approval.
class AutomationVaultSelection extends ConsumerStatefulWidget {
  const AutomationVaultSelection({
    super.key,
    required this.scope,
    required this.projectId,
    required this.service,
    required this.onChanged,
    this.manualPassword,
    this.onManualChanged,
    this.busy = false,
  });
  final AutomationScope scope;
  final String projectId;
  final AutomationVaultSelectionService service;
  final void Function(bool enabled, AutomationJson? selection) onChanged;
  final TextEditingController? manualPassword;
  final VoidCallback? onManualChanged;
  final bool busy;
  @override
  ConsumerState<AutomationVaultSelection> createState() => _AutomationVaultSelectionState();
}

class _AutomationVaultSelectionState extends ConsumerState<AutomationVaultSelection> {
  bool enabled = false, loading = false, manual = false;
  String? credential, savedCredential;
  AutomationJson? frame;
  Object? error;
  int generation = 0;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get busy => loading || widget.busy;

  AutomationJson checkContext(AutomationJson raw) {
    final value = raw['frame'];
    const fields = {
      'version',
      'profileId',
      'notificationScopeId',
      'projectId',
      'projectRevision',
      'catalogSha256',
      'revisionDigest',
      'directoryIdentity',
      'referenceSha256',
    };
    if (raw['version'] != 1 ||
        value is! AutomationJson ||
        value.length != fields.length ||
        !value.keys.every(fields.contains) ||
        value['version'] != 1 ||
        value['profileId'] != widget.scope.profileId.toString() ||
        value['projectId'] != widget.projectId ||
        value['projectRevision'] is! int ||
        (value['projectRevision'] as int) <= 0 ||
        value['notificationScopeId'] is! String ||
        (value['notificationScopeId'] as String).isEmpty) {
      throw const FormatException('Invalid Vault selection context');
    }
    return Map<String, dynamic>.unmodifiable(value);
  }

  Future<void> load() async {
    final attempt = ++generation;
    setState(() {
      loading = true;
      error = null;
      frame = null;
      savedCredential = null;
    });
    widget.onChanged(true, null);
    try {
      final result = await widget.service.vaultContext(widget.projectId);
      if (!current || !enabled || attempt != generation) return;
      final captured = checkContext(result);
      setState(() => frame = captured);
      // An existing reference is deliberately not automatic credential consent.
    } catch (e) {
      if (current && enabled && attempt == generation) setState(() => error = e);
    } finally {
      if (current && attempt == generation) setState(() => loading = false);
    }
  }

  Future<void> save() async {
    final captured = frame, selected = credential;
    if (!current || busy || !enabled || captured == null || selected == null) return;
    final attempt = ++generation;
    setState(() {
      loading = true;
      error = null;
      savedCredential = null;
    });
    widget.onChanged(true, null);
    try {
      final result = await widget.service.selectVault({'frame': captured, 'credentialId': selected});
      if (!current || !enabled || attempt != generation) return;
      final fresh = checkContext(result);
      if (result['referencedCredentialId'] != selected) throw const FormatException('Vault selection changed');
      setState(() {
        frame = fresh;
        savedCredential = selected;
      });
      widget.onChanged(true, {'vault_credential_id': selected, 'vault_selection': fresh});
    } catch (e) {
      if (current && enabled && attempt == generation) setState(() => error = e);
    } finally {
      if (current && attempt == generation) setState(() => loading = false);
    }
  }

  Future<void> create() async {
    if (!current || busy) return;
    final created = await showAppDialog<Credential>(
      context,
      secure: true,
      builder: (_) => AutomationScopeGuard(scope: widget.scope, builder: (_) => const PasswordCredentialDialog()),
    );
    if (!current || created == null) return;
    final refreshed = await ref.refresh(credentialsProvider.future);
    if (!current || !enabled) return;
    if (!refreshed.any((c) => c.id == created.id && c.kind == CredentialKind.password && c.secretId != null)) return;
    setState(() {
      credential = created.id.value;
      savedCredential = null;
    });
    widget.onChanged(true, null);
  }

  @override
  Widget build(BuildContext context) {
    if (!identical(ref.watch(automationScopeProvider), widget.scope)) return const SizedBox.shrink();
    final l = context.l10n;
    final credentials = enabled && !manual ? ref.watch(credentialsProvider) : null;
    final eligible = (credentials?.value ?? const <Credential>[])
        .where((c) => c.kind == CredentialKind.password && c.secretId != null)
        .toList();
    final selected = eligible.any((c) => c.id.value == credential) ? credential : null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        CheckboxListTile(
          key: const ValueKey('automation-vault-enable'),
          dense: true,
          value: enabled,
          title: Text(l.automationVaultUse),
          onChanged: widget.busy
              ? null
              : (value) {
                  setState(() {
                    enabled = value == true;
                    error = null;
                  });
                  if (enabled) {
                    load();
                  } else {
                    generation++;
                    widget.manualPassword?.clear();
                    setState(() {
                      manual = false;
                      loading = false;
                      frame = null;
                      savedCredential = null;
                      credential = null;
                    });
                    widget.onChanged(false, null);
                  }
                },
        ),
        if (enabled) ...[
          if (widget.manualPassword != null) ...[
            GlassSelect<bool>(
              key: const ValueKey('automation-vault-password-source'),
              value: manual,
              expand: true,
              semanticLabel: l.automationVaultPasswordSource,
              items: [
                GlassSelectItem(value: false, label: l.automationVaultSavedPassword),
                GlassSelectItem(value: true, label: l.automationVaultManual),
              ],
              onChanged: busy
                  ? null
                  : (value) {
                      widget.manualPassword!.clear();
                      setState(() {
                        manual = value;
                        savedCredential = null;
                      });
                      widget.onChanged(true, null);
                    },
            ),
            const SizedBox(height: 8),
          ],
          if (manual) ...[
            if (frame?['referenceSha256'] != null)
              Text(l.automationVaultManualLinked)
            else
              SecretField(
                key: const ValueKey('automation-vault-manual-password'),
                controller: widget.manualPassword!,
                label: l.automationVaultPassword,
                helper: l.automationVaultManualRunHint,
                enabled: !busy,
                onChanged: (_) => widget.onManualChanged?.call(),
              ),
          ] else ...[
            Text(l.automationVaultDisclosure),
            const SizedBox(height: 8),
            if (loading || credentials?.isLoading == true) const LinearProgressIndicator(),
            GlassSelect<String?>(
              key: const ValueKey('automation-vault-credential'),
              value: selected,
              expand: true,
              semanticLabel: l.automationVaultChoose,
              items: [
                GlassSelectItem(value: null, label: l.automationVaultChoose),
                for (final c in eligible) GlassSelectItem(value: c.id.value, label: c.name),
              ],
              onChanged: busy
                  ? null
                  : (value) {
                      setState(() {
                        credential = value;
                        savedCredential = null;
                      });
                      widget.onChanged(true, null);
                    },
            ),
            const SizedBox(height: 8),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                OutlinedButton(onPressed: busy ? null : create, child: Text(l.automationVaultAdd)),
                OutlinedButton(
                  key: const ValueKey('automation-vault-save'),
                  onPressed: busy || frame == null || selected == null ? null : save,
                  child: Text(l.automationVaultConnect),
                ),
                if (error != null) TextButton(onPressed: busy ? null : load, child: Text(l.commonRetry)),
              ],
            ),
            if (savedCredential != null && savedCredential == selected) Text(l.automationVaultConnected),
            if (error != null || credentials?.hasError == true)
              InfoBanner(tone: BannerTone.danger, message: errorMessage(l, error ?? credentials!.error!)),
          ],
        ],
      ],
    );
  }
}
