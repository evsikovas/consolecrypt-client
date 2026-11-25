import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/editor/workspace_code_editor.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class EditorSettingsSection extends ConsumerWidget {
  const EditorSettingsSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.read(settingsServiceProvider);
    final local = ref.watch(localSettingsProvider).value ?? settings.currentLocal;
    final font = ref.watch(editorFontProvider), l = context.l10n, t = GlassTokens.of(context);
    return SectionCard(
      title: l.editorTitle,
      icon: Icons.code_rounded,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          LabeledValue(
            label: l.settingsTerminalFontLabel,
            value: Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassSelect<String>(
                key: const ValueKey('editor-font-family'),
                value: local.editorFontFamily ?? '',
                semanticLabel: l.settingsTerminalFontLabel,
                items: [
                  GlassSelectItem(value: '', label: l.settingsTerminalFontSystem),
                  for (final family in {
                    ...ref.watch(terminalFontCatalogProvider).value?.offered ?? AppPlatform.terminalFontChoices,
                    ?local.editorFontFamily,
                  })
                    GlassSelectItem(value: family, label: family),
                ],
                onChanged: (value) => settings.updateLocal(
                  settings.currentLocal.copyWith(editorFontFamily: value, resetEditorFontFamily: value.isEmpty),
                ),
              ),
            ),
          ),
          LabeledValue(
            label: l.settingsFontSizeLabel,
            value: Row(
              children: [
                Expanded(
                  child: Slider(
                    key: const ValueKey('editor-font-size'),
                    value: local.editorFontSize,
                    min: LocalSettings.editorMinFontSize,
                    max: LocalSettings.editorMaxFontSize,
                    divisions: 24,
                    onChanged: (v) => settings.updateLocal(settings.currentLocal.copyWith(editorFontSize: v)),
                  ),
                ),
                SizedBox(width: 32, child: Text(local.editorFontSize.round().toString(), textAlign: TextAlign.end)),
              ],
            ),
          ),
          Semantics(
            label: l.editorFontPreview,
            child: Container(
              padding: const EdgeInsets.all(16),
              decoration: BoxDecoration(color: t.surfaces.paper, borderRadius: BorderRadius.circular(8)),
              child: Text(
                'def listen(port):\n    return port or 8080',
                textDirection: TextDirection.ltr,
                style: t.typography.mono.copyWith(
                  fontFamily: font.family,
                  fontFamilyFallback: font.fallback,
                  fontSize: local.editorFontSize,
                  height: 1.5,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
