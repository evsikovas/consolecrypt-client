import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:material_ui/material_ui.dart';

/// Dialog/launch-local memory only, deliberately no JSON, persistence or Clone.
final class AutomationLaunchPasswords {
  AutomationLaunchPasswords({this.vaultPassword, this.becomePassword, this.useProjectVaultPassword = false});
  final SecretText? vaultPassword, becomePassword;
  final bool useProjectVaultPassword;
  bool get hasVault => useProjectVaultPassword || vaultPassword?.isNotEmpty == true && !vaultPassword!.isWiped;
  bool get hasAny => hasVault || becomePassword?.isNotEmpty == true && !becomePassword!.isWiped;
  void wipe() {
    vaultPassword?.wipe();
    becomePassword?.wipe();
  }

  @override
  String toString() => 'AutomationLaunchPasswords(<redacted>)';
}

Future<AutomationLaunchPasswords?> showAutomationLaunchPasswords(
  BuildContext context, {
  required bool Function() isCurrent,
  required bool allowVault,
}) async {
  if (!isCurrent()) return null;
  final result = await showAppDialog<AutomationLaunchPasswords>(
    context,
    secure: true,
    builder: (_) => _Passwords(isCurrent: isCurrent, allowVault: allowVault),
  );
  if (!isCurrent()) {
    result?.wipe();
    return null;
  }
  return result;
}

class _Passwords extends StatefulWidget {
  const _Passwords({required this.isCurrent, required this.allowVault});
  final bool Function() isCurrent;
  final bool allowVault;
  @override
  State<_Passwords> createState() => _State();
}

class _State extends State<_Passwords> {
  final vault = TextEditingController(), become = TextEditingController();
  bool useVault = false, useFile = false, useBecome = false;
  bool get valid => (!useVault || useFile || vault.text.isNotEmpty) && (!useBecome || become.text.isNotEmpty);
  @override
  void dispose() {
    vault.clear();
    become.clear();
    vault.dispose();
    become.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (!widget.isCurrent()) return const SizedBox.shrink();
    return AlertDialog(
      key: const ValueKey('automation-launch-passwords'),
      title: Text(l.automationRunPasswords),
      content: SizedBox(
        width: 500,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.automationRunPasswordsHint),
              if (widget.allowVault) ...[
                CheckboxListTile(
                  key: const ValueKey('automation-launch-vault-enable'),
                  value: useVault,
                  title: Text(l.automationVaultUse),
                  onChanged: (value) => setState(() {
                    useVault = value == true;
                    vault.clear();
                  }),
                ),
                if (useVault) ...[
                  CheckboxListTile(
                    key: const ValueKey('automation-launch-vault-file'),
                    value: useFile,
                    title: Text(l.automationVaultProjectFile),
                    onChanged: (value) => setState(() {
                      useFile = value == true;
                      vault.clear();
                    }),
                  ),
                  if (!useFile)
                    SecretField(
                      key: const ValueKey('automation-launch-vault-password'),
                      controller: vault,
                      label: l.automationVaultPassword,
                      onChanged: (_) => setState(() {}),
                    ),
                ],
              ],
              CheckboxListTile(
                key: const ValueKey('automation-launch-become-enable'),
                value: useBecome,
                title: Text(l.automationUseBecomePassword),
                onChanged: (value) => setState(() {
                  useBecome = value == true;
                  become.clear();
                }),
              ),
              if (useBecome)
                SecretField(
                  key: const ValueKey('automation-launch-become-password'),
                  controller: become,
                  label: l.automationBecomePassword,
                  onChanged: (_) => setState(() {}),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(l.commonCancel)),
        FilledButton(
          key: const ValueKey('automation-launch-passwords-apply'),
          onPressed: !valid
              ? null
              : () {
                  if (!widget.isCurrent()) return;
                  final result = AutomationLaunchPasswords(
                    vaultPassword: useVault && !useFile ? SecretText(vault.text) : null,
                    becomePassword: useBecome ? SecretText(become.text) : null,
                    useProjectVaultPassword: useVault && useFile,
                  );
                  vault.clear();
                  become.clear();
                  Navigator.of(context).pop(result);
                },
          child: Text(l.commonContinue),
        ),
      ],
    );
  }
}
