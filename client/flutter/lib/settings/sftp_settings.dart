import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class SftpSettingsSection extends ConsumerStatefulWidget {
  const SftpSettingsSection({super.key});

  @override
  ConsumerState<SftpSettingsSection> createState() => _SftpSettingsSectionState();
}

class _SftpSettingsSectionState extends ConsumerState<SftpSettingsSection> {
  bool _busy = false;

  Future<void> _choose() async {
    final l = context.l10n;
    final dialogs = ref.read(fileDialogServiceProvider);
    final settings = ref.read(settingsServiceProvider);
    setState(() => _busy = true);
    try {
      await runWithFeedback(context, () async {
        final path = await dialogs.chooseApplication(
          label: l.sftpApplications,
          confirmButtonText: l.sftpChooseApplication,
        );
        if (path == null || !mounted) return;
        await settings.updateLocal(settings.currentLocal.copyWith(sftpDefaultEditor: AppRef(AppRefKind.path, path)));
      });
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final settings = ref.read(settingsServiceProvider);
    final local = ref.watch(localSettingsProvider).value ?? settings.currentLocal;
    final editor = local.sftpDefaultEditor;
    return SectionCard(
      key: const ValueKey('settings-sftp'),
      title: l.sftpDefaultEditorTitle,
      icon: Icons.folder_open_rounded,
      subtitle: AppPlatform.isMobile ? null : l.sftpDefaultEditorHelp,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GlassSegmented<bool>(
            semanticLabel: l.sftpDefaultEditor,
            inChrome: false,
            selected: local.sftpUseBuiltinEditor,
            segments: [
              GlassSegment(value: true, label: l.sftpBuiltinEditor, key: const ValueKey('sftp-editor-builtin')),
              GlassSegment(
                value: false,
                label: AppPlatform.isMobile ? l.sftpToolbarQuickLook : l.sftpExternalEditor,
                key: const ValueKey('sftp-editor-external'),
              ),
            ],
            onChanged: (value) => runWithFeedback(
              context,
              () => settings.updateLocal(settings.currentLocal.copyWith(sftpUseBuiltinEditor: value)),
            ),
          ),
          if (!AppPlatform.isMobile && !local.sftpUseBuiltinEditor) ...[
            const SizedBox(height: GlassSpacing.s12),
            LabeledValue(
              label: l.sftpDefaultEditor,
              value: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(editor?.displayName ?? l.sftpSystemEditor, key: const ValueKey('sftp-default-editor-name')),
                  if (editor != null) Text(editor.value, style: Theme.of(context).textTheme.bodySmall),
                ],
              ),
            ),
            const SizedBox(height: GlassSpacing.s12),
            Wrap(
              spacing: GlassSpacing.s8,
              runSpacing: GlassSpacing.s8,
              children: [
                GlassButton(
                  key: const ValueKey('sftp-default-editor-choose'),
                  busy: _busy,
                  icon: Icons.apps_rounded,
                  label: l.sftpChooseApplication,
                  onPressed: _busy ? null : _choose,
                ),
                if (editor != null)
                  GlassButton.plain(
                    key: const ValueKey('sftp-default-editor-reset'),
                    icon: Icons.restore_rounded,
                    label: l.sftpSystemEditor,
                    onPressed: _busy
                        ? null
                        : () => runWithFeedback(
                            context,
                            () => settings.updateLocal(settings.currentLocal.copyWith(resetSftpDefaultEditor: true)),
                          ),
                  ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}
