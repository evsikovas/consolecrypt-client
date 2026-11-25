import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final editorFontProvider = Provider<TerminalFont>(
  (ref) => resolveTerminalFont(
    ref.watch(localSettingsProvider.select((value) => value.value?.editorFontFamily)),
    ref.watch(terminalFontCatalogProvider).value,
  ),
);

/// Both workspaces supply their own canonical buffer and clipboard policy.
class WorkspaceCodeEditor extends ConsumerWidget {
  const WorkspaceCodeEditor({
    super.key,
    required this.controller,
    required this.path,
    required this.onCopySelection,
    this.readOnly = false,
    this.onChanged,
    this.foldTooltip,
    this.limitMessage,
    required this.fieldKey,
    required this.maxBytes,
  });
  final TextEditingController controller;
  final String? path, limitMessage;
  final bool readOnly;
  final int maxBytes;
  final Key fieldKey;
  final ValueChanged<String>? onChanged;
  final String Function(bool, int)? foldTooltip;
  final Future<bool> Function(String) onCopySelection;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.read(settingsServiceProvider);
    final size =
        ref.watch(localSettingsProvider.select((s) => s.value?.editorFontSize)) ?? settings.currentLocal.editorFontSize;
    final font = ref.watch(editorFontProvider);
    return AnsibleCodeEditor(
      controller: controller,
      path: path,
      fieldKey: fieldKey,
      readOnly: readOnly,
      onChanged: onChanged,
      onCopySelection: onCopySelection,
      foldTooltip: foldTooltip,
      limitMessage: limitMessage,
      maxBytes: maxBytes,
      fontSize: size,
      fontFamily: font.family,
      fontFallback: font.fallback,
      onZoom: (delta) {
        final current = settings.currentLocal;
        final next = (current.editorFontSize + delta).clamp(
          LocalSettings.editorMinFontSize,
          LocalSettings.editorMaxFontSize,
        );
        if (next != current.editorFontSize) settings.updateLocal(current.copyWith(editorFontSize: next));
      },
    );
  }
}
