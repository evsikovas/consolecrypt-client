import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/settings/color_picker.dart';
import 'package:consolecrypt/settings/terminal_theme_io.dart';
import 'package:file_selector/file_selector.dart';
import 'package:material_ui/material_ui.dart';

Future<TerminalColors?> editTerminalColors(BuildContext context, TerminalColors colors) =>
    showAppDialog<TerminalColors>(context, builder: (_) => TerminalThemeEditor(initial: colors));

class TerminalThemeEditor extends StatefulWidget {
  const TerminalThemeEditor({super.key, required this.initial});
  final TerminalColors initial;
  @override
  State<TerminalThemeEditor> createState() => _TerminalThemeEditorState();
}

class _TerminalThemeEditorState extends State<TerminalThemeEditor> {
  late TerminalColors _draft = widget.initial;
  bool _busy = false;
  String? _error;

  Future<void> _import() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final file = await openFile(
        acceptedTypeGroups: [
          XTypeGroup(
            label: context.l10n.terminalThemeFileType,
            extensions: ['json', 'itermcolors'],
            uniformTypeIdentifiers: ['public.json', 'public.xml'],
          ),
        ],
      );
      if (file == null) return;
      if (await file.length() > maxTerminalThemeBytes) throw const FormatException('Theme too large');
      final colors = parseTerminalTheme(await file.readAsString());
      if (mounted) setState(() => _draft = colors);
    } catch (_) {
      if (mounted) setState(() => _error = context.l10n.terminalThemeImportError);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _export() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final file = await getSaveLocation(
        suggestedName: 'ConsoleCrypt-theme.json',
        acceptedTypeGroups: [
          XTypeGroup(
            label: context.l10n.terminalThemeJsonType,
            extensions: ['json'],
            uniformTypeIdentifiers: ['public.json'],
          ),
        ],
      );
      if (file != null) {
        await XFile.fromData(
          Uint8List.fromList(utf8.encode(encodeTerminalTheme(_draft))),
          mimeType: 'application/json',
          name: 'ConsoleCrypt-theme.json',
        ).saveTo(file.path);
      }
    } catch (_) {
      if (mounted) setState(() => _error = context.l10n.terminalThemeExportError);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _pick(String key, String title) async {
    final value = await pickRgbColor(context, title: title, color: _draft[key]);
    if (value != null && mounted) setState(() => _draft = _draft.withColor(key, value));
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    String label(String key) => switch (key) {
      'background' => l.terminalColorBackground,
      'foreground' => l.terminalColorForeground,
      'cursor' => l.terminalColorCursor,
      'selection' => l.terminalColorSelection,
      _ =>
        int.parse(key.substring(4)) < 8
            ? l.terminalColorAnsi(int.parse(key.substring(4)))
            : l.terminalColorAnsiBright(int.parse(key.substring(4)) - 8),
    };
    return GlassDialog(
      key: const ValueKey('terminal-theme-editor'),
      title: l.terminalThemeEdit,
      width: 700,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TerminalPalettePreview(colors: _draft),
            const SizedBox(height: 16),
            Text(l.terminalThemeEditorHelp, style: tokens.typography.callout),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final key in TerminalColors.keys)
                  SizedBox(
                    width: 140,
                    child: GlassInteractive(
                      key: ValueKey('terminal-color-$key'),
                      semanticLabel: '${label(key)} ${TerminalColors.hex(_draft[key])}',
                      onPressed: _busy ? null : () => _pick(key, label(key)),
                      builder: (context, state) => GlassFocusRing(
                        visible: state.focusVisible,
                        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
                        child: Container(
                          padding: const EdgeInsets.all(8),
                          decoration: BoxDecoration(
                            border: Border.all(color: tokens.surfaces.hairlineCard),
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: Row(
                            children: [
                              Container(
                                width: 24,
                                height: 32,
                                decoration: BoxDecoration(
                                  color: Color(0xFF000000 | _draft[key]),
                                  borderRadius: BorderRadius.circular(5),
                                  border: Border.all(color: tokens.secondaryLabel),
                                ),
                              ),
                              const SizedBox(width: 8),
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(label(key), style: tokens.typography.caption, maxLines: 2),
                                    Text(
                                      TerminalColors.hex(_draft[key]),
                                      style: tokens.typography.caption.copyWith(
                                        color: tokens.secondaryLabel,
                                        fontFamily: AppPlatform.monospaceFamily,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                GlassButton(
                  key: const ValueKey('terminal-theme-import'),
                  icon: Icons.file_open_rounded,
                  label: l.terminalThemeImport,
                  onPressed: _busy ? null : _import,
                ),
                GlassButton(
                  key: const ValueKey('terminal-theme-export'),
                  icon: Icons.save_alt_rounded,
                  label: l.terminalThemeExport,
                  onPressed: _busy ? null : _export,
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(l.terminalThemeFormats, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
            if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!)),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('terminal-theme-cancel'),
          label: l.commonCancel,
          onPressed: _busy ? null : () => closeDialog<TerminalColors>(context),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('terminal-theme-save'),
        label: l.settingsColorApply,
        onPressed: _busy ? null : () => closeDialog(context, _draft),
      ),
    );
  }
}

// Technical preview data, never a command execution or real connection.
const _previewUser = 'user@host ';
const _previewDirectory = '~/projects ';
const _previewCommand = '% ls\nsrc/  README.md  build.sh '; // l10n-ignore: technical terminal sample.

class TerminalPalettePreview extends StatelessWidget {
  const TerminalPalettePreview({super.key, required this.colors});
  final TerminalColors colors;
  @override
  Widget build(BuildContext context) {
    final theme = customTerminalPalette(colors);
    return Container(
      key: const ValueKey('custom-terminal-preview'),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(color: theme.background, borderRadius: BorderRadius.circular(12)),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: _previewUser,
                  style: TextStyle(color: theme.green),
                ),
                TextSpan(
                  text: _previewDirectory,
                  style: TextStyle(color: theme.blue),
                ),
                TextSpan(
                  text: _previewCommand,
                  style: TextStyle(color: theme.foreground),
                ),
                WidgetSpan(child: Container(width: 8, height: 16, color: theme.cursor)),
              ],
            ),
            style: TextStyle(fontFamily: AppPlatform.monospaceFamily, fontSize: 13, height: 1.5),
          ),
          const SizedBox(height: 8),
          Text(
            context.l10n.terminalSampleSelection,
            style: TextStyle(
              fontFamily: AppPlatform.monospaceFamily,
              fontSize: 13,
              color: theme.foreground,
              backgroundColor: theme.selection,
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              for (var i = 0; i < 16; i++)
                Expanded(child: Container(height: 8, color: Color(0xFF000000 | colors['ansi$i']))),
            ],
          ),
        ],
      ),
    );
  }
}
