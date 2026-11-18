import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Transforms one already-saved YAML through Native CAS. Never returns the
/// password, reads .vault/password into Dart, or changes the run's extra vars.
Future<AutomationProjectFile?> showAutomationVaultFileDialog(
  BuildContext context,
  WidgetRef ref, {
  required AutomationScope scope,
  required AutomationProject project,
  required AutomationProjectFile file,
  required AutomationVaultFileService service,
  required bool Function() originalCurrent,
}) async {
  if (!originalCurrent() ||
      project.readOnly ||
      project.revision == null ||
      !(file.path.endsWith('.yml') || file.path.endsWith('.yaml'))) {
    return null;
  }
  final app = ref.read(appServicesProvider);
  bool current() =>
      context.mounted &&
      originalCurrent() &&
      identical(ref.read(appServicesProvider), app) &&
      identical(ref.read(automationScopeProvider), scope);
  final result = await showAppDialog<AutomationProjectFile>(
    context,
    secure: true,
    builder: (_) => AutomationScopeGuard(
      scope: scope,
      builder: (_) =>
          _VaultFileDialog(scope: scope, project: project, file: file, service: service, originalCurrent: current),
    ),
  );
  return current() ? result : null;
}

class _VaultFileDialog extends ConsumerStatefulWidget {
  const _VaultFileDialog({
    required this.scope,
    required this.project,
    required this.file,
    required this.service,
    required this.originalCurrent,
  });
  final AutomationScope scope;
  final AutomationProject project;
  final AutomationProjectFile file;
  final AutomationVaultFileService service;
  final bool Function() originalCurrent;
  @override
  ConsumerState<_VaultFileDialog> createState() => _State();
}

class _State extends ConsumerState<_VaultFileDialog> {
  final password = TextEditingController();
  bool useProjectFile = false, busy = false, loading = true;
  AutomationJson? frame;
  Object? error;
  bool get current => mounted && widget.originalCurrent();
  bool get decrypt => widget.file.text.startsWith(r'$ANSIBLE_VAULT;');

  @override
  void initState() {
    super.initState();
    load();
  }

  Future<void> load() async {
    try {
      final value = await widget.service.vaultFileContext(widget.project.id);
      if (!current) return;
      final candidate = value['frame'];
      if (candidate is! AutomationJson ||
          candidate['version'] != 1 ||
          candidate['profileId'] != widget.scope.profileId.toString() ||
          candidate['projectId'] != widget.project.id ||
          candidate['projectRevision'] != widget.project.revision ||
          candidate['notificationScopeId'] is! String ||
          (candidate['notificationScopeId'] as String).isEmpty) {
        throw const AutomationFileConflict();
      }
      setState(() {
        frame = Map.of(candidate);
        loading = false;
      });
    } catch (failure) {
      if (current) {
        setState(() {
          error = failure;
          loading = false;
        });
      }
    }
  }

  Future<void> submit() async {
    final original = frame;
    if (!current || busy || original == null || !useProjectFile && password.text.isEmpty) return;
    final secret = useProjectFile ? null : SecretText(password.text);
    password.clear();
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await widget.service.transformVaultFile({
        'version': 1,
        'profileId': original['profileId'],
        'notificationScopeId': original['notificationScopeId'],
        'projectId': widget.project.id,
        'projectRevision': widget.project.revision,
        'path': widget.file.path,
        'expectedSha256': widget.file.sha256,
        'action': decrypt ? 'decrypt' : 'encrypt',
        'passwordSource': useProjectFile ? 'projectFile' : 'manual',
      }, password: secret);
      if (!mounted || !current) return;
      if (result.path != widget.file.path ||
          result.sha256 == widget.file.sha256 ||
          result.text.startsWith(r'$ANSIBLE_VAULT;') == decrypt) {
        throw const AutomationFileConflict();
      }
      Navigator.of(context).pop(result);
    } catch (failure) {
      if (current) setState(() => error = failure);
    } finally {
      secret?.wipe();
      if (current) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    password.clear();
    password.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!current) return const SizedBox.shrink();
    return AlertDialog(
      key: const ValueKey('automation-vault-file-dialog'),
      title: Text(decrypt ? l.automationVaultDecrypt : l.automationVaultEncrypt),
      content: SizedBox(
        width: 500,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.file.path),
              const SizedBox(height: 12),
              Text(l.automationVaultFileHint),
              if (decrypt) Padding(padding: const EdgeInsets.only(top: 8), child: Text(l.automationVaultDecryptHint)),
              if (loading || busy) const LinearProgressIndicator(),
              const SizedBox(height: 12),
              GlassSelect<bool>(
                key: const ValueKey('automation-vault-file-source'),
                value: useProjectFile,
                semanticLabel: l.automationVaultPasswordSource,
                expand: true,
                items: [
                  GlassSelectItem(value: false, label: l.automationVaultManual),
                  GlassSelectItem(value: true, label: l.automationVaultProjectFile),
                ],
                onChanged: busy
                    ? null
                    : (value) => setState(() {
                        useProjectFile = value;
                        password.clear();
                      }),
              ),
              if (!useProjectFile) ...[
                const SizedBox(height: 12),
                SecretField(
                  key: const ValueKey('automation-vault-file-password'),
                  controller: password,
                  label: l.automationVaultPassword,
                  enabled: !busy,
                  onChanged: (_) => setState(() {}),
                ),
              ],
              if (error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: InfoBanner(tone: BannerTone.danger, message: errorMessage(l, error!)),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: busy ? null : () => Navigator.of(context).pop(), child: Text(l.commonCancel)),
        FilledButton(
          key: const ValueKey('automation-vault-file-confirm'),
          onPressed: busy || loading || frame == null || !useProjectFile && password.text.isEmpty ? null : submit,
          child: Text(decrypt ? l.automationVaultDecrypt : l.automationVaultEncrypt),
        ),
      ],
    );
  }
}
