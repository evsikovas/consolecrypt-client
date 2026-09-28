import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/theme/terminal_palettes.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/settings/color_picker.dart';
import 'package:consolecrypt/settings/terminal_theme_editor.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

// Technical example data, never an actual connection or command execution.
const _terminalExample = (user: 'user@host', directory: ' ~/projects', command: ' % ls\n', file: 'README.md  ');

class InterfaceAppearanceControls extends ConsumerWidget {
  const InterfaceAppearanceControls({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final help = tokens.typography.callout.copyWith(color: tokens.secondaryLabel);
    void save(LocalSettings value) {
      runWithFeedback(context, () => settings.updateLocal(value));
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        LabeledValue(
          label: l.settingsUiFontScale,
          value: Row(
            children: [
              Expanded(
                child: Slider(
                  key: const ValueKey('ui-font-scale'),
                  value: local.uiFontScale.clamp(LocalSettings.uiFontScaleMin, LocalSettings.uiFontScaleMax),
                  min: LocalSettings.uiFontScaleMin,
                  max: LocalSettings.uiFontScaleMax,
                  divisions: 12,
                  label: '${(local.uiFontScale * 100).round()}%',
                  onChanged: (value) => save(settings.currentLocal.copyWith(uiFontScale: value)),
                ),
              ),
              SizedBox(width: 48, child: Text('${(local.uiFontScale * 100).round()}%', textAlign: TextAlign.end)),
            ],
          ),
        ),
        Text(l.settingsUiScaleHelp, style: help),
        const SizedBox(height: 12),
        LabeledValue(
          label: l.settingsAccentColor,
          value: _ColorPreference(
            id: 'ui-accent',
            value: local.uiAccentColor ?? (GlassPalette.brandBlue.toARGB32() & 0xFFFFFF),
            onChanged: (value) => save(settings.currentLocal.copyWith(uiAccentColor: value)),
          ),
        ),
        const SizedBox(height: 8),
        LabeledValue(
          label: l.settingsBackgroundTint,
          value: _ColorPreference(
            id: 'ui-background',
            value: local.uiBackgroundColor ?? 0x8793A5,
            onChanged: (value) => save(settings.currentLocal.copyWith(uiBackgroundColor: value)),
          ),
        ),
        const SizedBox(height: 4),
        Text(l.settingsColorsHelp, style: help),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: GlassButton.plain(
            key: const ValueKey('reset-ui-appearance'),
            label: l.settingsResetUi,
            onPressed: () => save(settings.currentLocal.copyWith(uiFontScale: 1, resetUiColors: true)),
          ),
        ),
        CheckboxListTile(
          key: const ValueKey('settings-reopen-last-profile'),
          contentPadding: EdgeInsets.zero,
          value: local.reopenLastProfile,
          controlAffinity: ListTileControlAffinity.leading,
          title: Text(l.reopenLastProfile),
          subtitle: Text(l.reopenLastProfileHelp),
          onChanged: (value) => save(settings.currentLocal.copyWith(reopenLastProfile: value ?? false)),
        ),
        Text(l.settingsAppearanceLocalHint, style: help),
        const SizedBox(height: 12),
      ],
    );
  }
}

class _ColorPreference extends StatefulWidget {
  const _ColorPreference({required this.id, required this.value, required this.onChanged});
  final String id;
  final int value;
  final ValueChanged<int> onChanged;

  @override
  State<_ColorPreference> createState() => _ColorPreferenceState();
}

class _ColorPreferenceState extends State<_ColorPreference> {
  static const swatches = [
    0x4B89FF,
    0xFB7D00,
    0x22B8AD,
    0x57B870,
    0x9670F4,
    0xDF73A4,
    0xE96565,
    0x8793A5,
    0xFFFFFF,
    0x000000,
  ];
  String hex(int value) => '#${value.toRadixString(16).padLeft(6, '0').toUpperCase()}';
  late final _text = TextEditingController(text: hex(widget.value));
  bool _invalid = false;

  @override
  void didUpdateWidget(covariant _ColorPreference oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != oldWidget.value) {
      _text.text = hex(widget.value);
      _invalid = false;
    }
  }

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void apply() {
    final value = _text.text.trim();
    if (!RegExp(r'^#?[0-9a-fA-F]{6}$').hasMatch(value)) {
      setState(() => _invalid = true);
      return;
    }
    setState(() => _invalid = false);
    widget.onChanged(int.parse(value.replaceFirst('#', ''), radix: 16));
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            for (final value in swatches)
              Tooltip(
                message: hex(value),
                child: GlassInteractive(
                  key: ValueKey('${widget.id}-swatch-$value'),
                  semanticLabel: hex(value),
                  selected: value == widget.value,
                  onPressed: () => widget.onChanged(value),
                  builder: (context, state) => GlassFocusRing(
                    visible: state.focusVisible,
                    shape: const CircleBorder(),
                    child: Container(
                      width: 24,
                      height: 24,
                      decoration: BoxDecoration(
                        color: Color(0xFF000000 | value),
                        shape: BoxShape.circle,
                        border: Border.all(
                          color: value == widget.value ? tokens.palette.label : tokens.surfaces.hairlineCard,
                          width: value == widget.value ? 2 : 1,
                        ),
                      ),
                      child: value == widget.value
                          ? Icon(
                              Icons.check_rounded,
                              size: 14,
                              color: Color(0xFF000000 | value).computeLuminance() > .179
                                  ? const Color(0xFF000000)
                                  : const Color(0xFFFFFFFF),
                            )
                          : null,
                    ),
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            SizedBox(
              width: 142,
              child: TextField(
                key: ValueKey('${widget.id}-hex'),
                controller: _text,
                style: tokens.typography.mono,
                maxLength: 7,
                decoration: InputDecoration(labelText: l.settingsColorHex, counterText: ''),
                onSubmitted: (_) => apply(),
              ),
            ),
            GlassButton(
              icon: Icons.color_lens_outlined,
              label: l.colorSpectrum,
              onPressed: () async {
                final value = await pickRgbColor(context, title: l.colorSpectrum, color: widget.value);
                if (value != null && mounted) widget.onChanged(value);
              },
            ),
            GlassButton(key: ValueKey('${widget.id}-apply'), label: l.settingsColorApply, onPressed: apply),
          ],
        ),
        if (_invalid)
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Text(
              l.settingsColorInvalid,
              style: tokens.typography.callout.copyWith(color: tokens.palette.danger),
            ),
          ),
      ],
    );
  }
}

String terminalSchemeLabel(TerminalColorScheme scheme, AppLocalizations l) => switch (scheme) {
  TerminalColorScheme.system => l.settingsTerminalThemeSystem,
  TerminalColorScheme.dark => l.settingsTerminalThemeDark,
  TerminalColorScheme.light => l.settingsTerminalThemeLight,
  TerminalColorScheme.midnight => l.settingsTerminalThemeMidnight,
  TerminalColorScheme.ocean => l.settingsTerminalThemeOcean,
  TerminalColorScheme.forest => l.settingsTerminalThemeForest,
  TerminalColorScheme.custom => l.settingsTerminalThemeCustom,
  TerminalColorScheme.amber => l.settingsTerminalThemeAmber,
};

class TerminalAppearanceControls extends ConsumerWidget {
  const TerminalAppearanceControls({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final settings = ref.read(settingsServiceProvider);
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final theme = AppTheme.terminalTheme(
      tokens.brightness,
      scheme: local.terminalColorScheme,
      custom: local.customTerminalColors,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        LabeledValue(
          label: l.settingsTerminalColorScheme,
          value: GlassSelect<TerminalColorScheme>(
            key: const ValueKey('terminal-color-scheme'),
            expand: true,
            value: local.terminalColorScheme,
            items: [
              for (final scheme in TerminalColorScheme.values)
                GlassSelectItem(
                  key: ValueKey('terminal-scheme-${scheme.name}'),
                  value: scheme,
                  label: terminalSchemeLabel(scheme, l),
                ),
            ],
            onChanged: (scheme) => runWithFeedback(
              context,
              () => settings.updateLocal(settings.currentLocal.copyWith(terminalColorScheme: scheme)),
            ),
          ),
        ),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: GlassButton(
            key: const ValueKey('terminal-theme-edit'),
            icon: Icons.palette_outlined,
            label: l.terminalThemeEdit,
            onPressed: () async {
              final colors = await editTerminalColors(context, colorsFromTerminalTheme(theme));
              if (colors == null || !context.mounted) return;
              await runWithFeedback(
                context,
                () => settings.updateLocal(
                  settings.currentLocal.copyWith(
                    terminalColorScheme: TerminalColorScheme.custom,
                    customTerminalColors: colors,
                  ),
                ),
              );
            },
          ),
        ),
        const SizedBox(height: 8),
        Text(l.settingsTerminalThemeHelp, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        const SizedBox(height: 12),
        Semantics(
          label: l.settingsTerminalPreview,
          child: Container(
            key: const ValueKey('terminal-theme-preview'),
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(color: theme.background, borderRadius: BorderRadius.circular(12)),
            child: Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: _terminalExample.user,
                    style: TextStyle(color: theme.green),
                  ),
                  TextSpan(
                    text: _terminalExample.directory,
                    style: TextStyle(color: theme.blue),
                  ),
                  TextSpan(
                    text: _terminalExample.command,
                    style: TextStyle(color: theme.foreground),
                  ),
                  TextSpan(
                    text: 'src/  ',
                    style: TextStyle(color: theme.cyan),
                  ),
                  TextSpan(
                    text: _terminalExample.file,
                    style: TextStyle(color: theme.foreground),
                  ),
                  TextSpan(
                    text: 'build.sh',
                    style: TextStyle(color: theme.yellow),
                  ),
                ],
              ),
              style: TextStyle(
                fontFamily: AppPlatform.monospaceFamily,
                fontFamilyFallback: AppPlatform.monospaceFallback,
                fontSize: local.terminalFontSize,
                height: 1.5,
              ),
            ),
          ),
        ),
        const SizedBox(height: 12),
      ],
    );
  }
}
