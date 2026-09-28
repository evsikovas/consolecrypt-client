import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:consolecrypt/core/widgets/obscurable_text_controller.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

enum _UnlockWith { passphrase, recoveryKey }

/// Restores a `.ccbackup` into a new local profile (ADR-0106). Nothing
/// existing is overwritten.
class RestoreBackupScreen extends ConsumerStatefulWidget {
  const RestoreBackupScreen({super.key});

  @override
  ConsumerState<RestoreBackupScreen> createState() => _RestoreBackupScreenState();
}

class _RestoreBackupScreenState extends ConsumerState<RestoreBackupScreen> {
  final _path = TextEditingController();
  final _passphrase = TextEditingController();
  final _recovery = ObscurableTextController();
  final _newPassphrase = TextEditingController();
  final _name = TextEditingController();
  _UnlockWith _with = _UnlockWith.passphrase;
  BackupInfo? _info;
  PassphraseStrength _strength = PassphraseStrength.estimate('');
  bool _busy = false;

  /// A service error; turned into localized text in build.
  AppException? _error;

  @override
  void dispose() {
    for (final c in [_path, _passphrase, _recovery, _newPassphrase, _name]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _browse() async {
    final path = await ref.read(fileDialogServiceProvider).chooseOpenFile(extensions: const [backupFileExtension]);
    if (path == null) return;
    _path.text = path;
    await _inspect();
  }

  Future<void> _inspect() async {
    setState(() {
      _error = null;
      _info = null;
    });
    try {
      final info = await ref.read(backupServiceProvider).inspectBackup(_path.text);
      if (mounted) setState(() => _info = info);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    }
  }

  bool get _canRestore {
    if (_info == null || _busy) return false;
    return switch (_with) {
      _UnlockWith.passphrase => _passphrase.text.isNotEmpty,
      _UnlockWith.recoveryKey => RecoveryInput.parse(_recovery.text) != null && _strength.acceptable,
    };
  }

  Future<void> _restore() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    final BackupUnlock unlock = switch (_with) {
      _UnlockWith.passphrase => BackupUnlockWithPassphrase(SecretText(_passphrase.text)),
      _UnlockWith.recoveryKey => BackupUnlockWithRecoveryKey(
        recoveryInput: SecretText(_recovery.text),
        newPassphrase: SecretText(_newPassphrase.text),
      ),
    };
    try {
      await ref.read(backupServiceProvider).restoreBackup(path: _path.text, unlock: unlock, profileName: _name.text);
      if (mounted) context.go(AppRoutes.hosts);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      switch (unlock) {
        case BackupUnlockWithPassphrase(:final passphrase):
          passphrase.wipe();
        case BackupUnlockWithRecoveryKey(:final recoveryInput, :final newPassphrase):
          recoveryInput.wipe();
          newPassphrase.wipe();
      }
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final hasProfiles = (ref.watch(profilesProvider).value?.profiles ?? const []).isNotEmpty;
    return GateScaffold(
      maxWidth: 620,
      leading: GlassButton(
        onPressed: () => context.go(hasProfiles ? AppRoutes.hosts : AppRoutes.welcome),
        icon: Icons.arrow_back_rounded,
        label: l10n.commonBack,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.settings_backup_restore_rounded,
            title: l10n.restoreTitle,
            subtitle: l10n.restoreSubtitle,
          ),
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('restore-path'),
                  controller: _path,
                  decoration: InputDecoration(labelText: l10n.restoreFileLabel),
                  onSubmitted: (_) => _inspect(),
                ),
              ),
              const SizedBox(width: GlassSpacing.s8),
              GlassButton(onPressed: _browse, icon: Icons.folder_open_rounded, label: l10n.restoreBrowse),
            ],
          ),
          if (_info != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s8),
              child: Column(
                children: [
                  LabeledValue(
                    label: l10n.restoreVaultIdLabel,
                    value: Text(
                      _info!.vaultId.value,
                      style: t.mono.copyWith(fontSize: 12, color: tokens.palette.label),
                    ),
                  ),
                  LabeledValue(label: l10n.restoreCreatedLabel, value: Text(formatDateTime(l10n, _info!.createdAt))),
                  LabeledValue(
                    label: l10n.restoreObjectsLabel,
                    value: Text(
                      l10n.restoreObjectsValue(
                        formatCount(l10n, _info!.objectCount),
                        formatBytes(l10n, _info!.sizeBytes),
                      ),
                    ),
                  ),
                  LabeledValue(
                    label: l10n.restoreWrittenByLabel,
                    value: Text(l10n.restoreWrittenByValue(_info!.appVersion, _info!.formatVersion)),
                  ),
                ],
              ),
            ),
            const SizedBox(height: GlassSpacing.s16),
            GlassSegmented<_UnlockWith>(
              key: const ValueKey('restore-unlock-with'),
              inChrome: false,
              expand: true,
              segments: [
                GlassSegment(value: _UnlockWith.passphrase, label: l10n.restoreUnlockPassphrase),
                GlassSegment(value: _UnlockWith.recoveryKey, label: l10n.restoreUnlockRecoveryKey),
              ],
              selected: _with,
              onChanged: (w) => setState(() => _with = w),
            ),
            const SizedBox(height: GlassSpacing.s12),
            if (_with == _UnlockWith.passphrase)
              SecretField(
                key: const ValueKey('restore-passphrase'),
                controller: _passphrase,
                label: l10n.restorePassphraseLabel,
                onChanged: (_) => setState(() {}),
              )
            else ...[
              // Recovery words are drawn as bullets until revealed (never blurred).
              TextField(
                controller: _recovery,
                minLines: 2,
                maxLines: 4,
                autocorrect: false,
                enableSuggestions: false,
                enableIMEPersonalizedLearning: false,
                style: t.mono.copyWith(color: tokens.palette.label),
                decoration: InputDecoration(
                  labelText: l10n.restoreRecoveryInputLabel,
                  suffixIcon: IconButton(
                    tooltip: _recovery.obscured ? l10n.commonShow : l10n.commonHide,
                    icon: Icon(_recovery.obscured ? Icons.visibility_rounded : Icons.visibility_off_rounded, size: 18),
                    onPressed: () => setState(() => _recovery.obscured = !_recovery.obscured),
                  ),
                ),
                onChanged: (_) => setState(() {}),
              ),
              const SizedBox(height: GlassSpacing.s12),
              SecretField(
                controller: _newPassphrase,
                label: l10n.restoreNewPassphraseLabel,
                onChanged: (v) => setState(() => _strength = PassphraseStrength.estimate(v)),
              ),
              const SizedBox(height: GlassSpacing.s8),
              StrengthMeter(strength: _strength),
              PassphraseLayoutNotice(passphrase: _newPassphrase.text),
            ],
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              controller: _name,
              decoration: InputDecoration(
                labelText: l10n.restoreProfileNameLabel,
                hintText: l10n.restoreProfileNameHint,
              ),
            ),
          ],
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
          const SizedBox(height: GlassSpacing.s20),
          GlassButton.prominent(
            key: const ValueKey('restore-submit'),
            size: GlassControlSize.lg,
            expand: true,
            busy: _busy,
            onPressed: _canRestore ? _restore : (_info == null ? _inspect : null),
            label: _info == null ? l10n.restoreOpenBackup : l10n.restoreIntoNewProfile,
          ),
          const DemoHints(),
        ],
      ),
    );
  }
}
