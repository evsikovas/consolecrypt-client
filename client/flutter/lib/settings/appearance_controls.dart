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
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:consolecrypt/terminal/terminal_status_bar.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

// Technical example data, never an actual connection or command execution.
const _terminalExample = (user: 'user@host', directory: ' ~/projects', command: ' % ls\n', file: 'README.md  ');

class InterfaceAppearanceControls extends ConsumerWidget {
  const InterfaceAppearanceControls({this.showProfilePreference = true, super.key});

  final bool showProfilePreference;

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
        if (showProfilePreference)
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

/// Terminal tabs above the terminal or in a column at its left edge
/// (desktop only; shown under Terminal and Appearance). [id] keys each copy.
class TerminalTabPlacementSetting extends ConsumerWidget {
  const TerminalTabPlacementSetting({required this.id, super.key});

  final String id;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final placement =
        ref.watch(localSettingsProvider.select((s) => s.value?.terminalTabPlacement)) ?? TerminalTabPlacement.top;
    final settings = ref.read(settingsServiceProvider);
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final groupSessions = CheckboxListTile(
      key: ValueKey('$id-grouped'),
      contentPadding: EdgeInsets.zero,
      dense: true,
      controlAffinity: ListTileControlAffinity.leading,
      title: Text(l.sessionGroupTabs),
      value: ref.watch(localSettingsProvider.select((s) => s.value?.terminalTabGrouped)) ?? false,
      onChanged: (value) => runWithFeedback(
        context,
        () => settings.updateLocal(settings.currentLocal.copyWith(terminalTabGrouped: value ?? false)),
      ),
    );
    if (AppPlatform.isMobile) return groupSessions;
    return LabeledValue(
      label: l.settingsTerminalTabPlacement,
      value: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          GlassSegmented<TerminalTabPlacement>(
            key: ValueKey(id),
            inChrome: false,
            semanticLabel: l.settingsTerminalTabPlacement,
            segments: [
              GlassSegment(
                key: ValueKey('$id-top'),
                value: TerminalTabPlacement.top,
                label: l.settingsTerminalTabPlacementTop,
              ),
              GlassSegment(
                key: ValueKey('$id-left'),
                value: TerminalTabPlacement.left,
                label: l.settingsTerminalTabPlacementLeft,
              ),
            ],
            selected: placement,
            onChanged: (value) => runWithFeedback(
              context,
              () => settings.updateLocal(settings.currentLocal.copyWith(terminalTabPlacement: value)),
            ),
          ),
          const SizedBox(height: GlassSpacing.s4),
          if (placement == TerminalTabPlacement.left) groupSessions,
          Text(
            l.settingsTerminalTabPlacementHelp,
            style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
          ),
        ],
      ),
    );
  }
}

class TerminalAppearanceControls extends ConsumerWidget {
  const TerminalAppearanceControls({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final font = ref.watch(terminalFontProvider);
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
                fontFamily: font.family,
                fontFamilyFallback: font.fallback,
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

class TerminalDeviceControls extends ConsumerWidget {
  const TerminalDeviceControls({this.showServerMetrics = true, super.key});
  final bool showServerMetrics;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final settings = ref.watch(settingsServiceProvider);
    final local = ref.watch(localSettingsProvider).value ?? settings.currentLocal;
    final terminalFont = ref.watch(terminalFontProvider);
    final foreground = AppTheme.terminalTheme(
      GlassTokens.of(context).brightness,
      scheme: local.terminalColorScheme,
      custom: local.customTerminalColors,
    ).foreground;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        LabeledValue(
          label: l10n.settingsFontSizeLabel,
          value: Slider(
            key: const ValueKey('terminal-font-size'),
            value: local.terminalFontSize.clamp(10, 22),
            min: 10,
            max: 22,
            divisions: 12,
            label: local.terminalFontSize.toStringAsFixed(0),
            onChanged: (v) => settings.updateLocal(local.copyWith(terminalFontSize: v)),
          ),
        ),
        LabeledValue(
          label: l10n.settingsTerminalFontLabel,
          value: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              GlassSelect<String>(
                key: const ValueKey('terminal-font-family'),
                value: local.terminalFontFamily ?? '',
                semanticLabel: l10n.settingsTerminalFontLabel,
                items: [
                  GlassSelectItem(value: '', label: l10n.settingsTerminalFontSystem),
                  for (final family in {
                    ...ref.watch(terminalFontCatalogProvider).value?.offered ?? AppPlatform.terminalFontChoices,
                    ?local.terminalFontFamily,
                  })
                    GlassSelectItem(key: ValueKey('terminal-font-$family'), value: family, label: family),
                  GlassSelectItem(
                    key: const ValueKey('terminal-font-custom'),
                    value: '\u0000custom',
                    label: l10n.settingsTerminalFontCustom,
                    icon: Icons.edit_outlined,
                  ),
                ],
                onChanged: (value) async {
                  if (value == '\u0000custom') {
                    final family = await showTextInputDialog(
                      context,
                      title: l10n.settingsTerminalFontCustomTitle,
                      initial: local.terminalFontFamily ?? '',
                      label: l10n.settingsTerminalFontCustomHint,
                    );
                    final name = family?.trim() ?? '';
                    if (name.isEmpty || name.length > 80) return;
                    await settings.updateLocal(settings.currentLocal.copyWith(terminalFontFamily: name));
                    return;
                  }
                  await settings.updateLocal(
                    value.isEmpty
                        ? settings.currentLocal.copyWith(resetTerminalFontFamily: true)
                        : settings.currentLocal.copyWith(terminalFontFamily: value),
                  );
                },
              ),
              if (terminalFont.missing) ...[
                const SizedBox(height: GlassSpacing.s4),
                Text(
                  l10n.settingsTerminalFontMissing(terminalFont.requested ?? '', terminalFont.fallbackFamily),
                  key: const ValueKey('terminal-font-missing'),
                  style: GlassTokens.of(context).typography.callout
                      .copyWith(color: GlassTokens.of(context).secondaryLabel),
                ),
              ],
            ],
          ),
        ),
        CheckboxListTile(
          key: const ValueKey('terminal-allow-multiline-paste'),
          contentPadding: EdgeInsets.zero,
          controlAffinity: ListTileControlAffinity.leading,
          title: Text(l10n.terminalAllowMultilinePaste),
          value: local.allowMultilinePaste,
          onChanged: (v) => runWithFeedback(
            context,
            () => settings.updateLocal(settings.currentLocal.copyWith(allowMultilinePaste: v)),
          ),
        ),
        if (showServerMetrics)
          SwitchListTile.adaptive(
            key: const ValueKey('terminal-metrics-setting'),
            contentPadding: EdgeInsets.zero,
            title: Text(l10n.terminalMetricsBar),
            value: local.terminalMetricsBar,
            onChanged: (v) => runWithFeedback(
              context,
              () => settings.updateLocal(settings.currentLocal.copyWith(terminalMetricsBar: v)),
            ),
          ),
        if (showServerMetrics && local.terminalMetricsBar)
          ExpansionTile(
            key: const ValueKey('terminal-bar-editor'),
            tilePadding: EdgeInsets.zero,
            title: Text(l10n.terminalBarItems),
            children: [
              for (final metric in TerminalMetric.values)
                Row(
                  children: [
                    Expanded(
                      child: CheckboxListTile(
                        key: ValueKey('bar-visible-${metric.name}'),
                        dense: true,
                        contentPadding: EdgeInsets.zero,
                        controlAffinity: ListTileControlAffinity.leading,
                        title: Text(metric.label(context)),
                        value: !local.hiddenTerminalMetrics.contains(metric),
                        onChanged: (enabled) {
                          final current = settings.currentLocal;
                          final hidden = {...current.hiddenTerminalMetrics};
                          if (enabled == true) {
                            hidden.remove(metric);
                          } else {
                            hidden.add(metric);
                          }
                          runWithFeedback(
                            context,
                            () => settings.updateLocal(current.copyWith(hiddenTerminalMetrics: hidden)),
                          );
                        },
                      ),
                    ),
                    IconButton(
                      key: ValueKey('bar-color-${metric.name}'),
                      tooltip: '${l10n.terminalBarColor} · ${metric.label(context)}',
                      icon: Icon(
                        Icons.palette_outlined,
                        color: local.terminalMetricColors.containsKey(metric)
                            ? Color(0xff000000 | local.terminalMetricColors[metric]!)
                            : null,
                      ),
                      onPressed: () async {
                        final color = await pickRgbColor(
                          context,
                          title: '${l10n.terminalBarColor} · ${metric.label(context)}',
                          color: local.terminalMetricColors[metric] ?? (foreground.toARGB32() & 0xffffff),
                        );
                        if (color == null || !context.mounted) return;
                        final current = settings.currentLocal;
                        await runWithFeedback(
                          context,
                          () => settings.updateLocal(
                            current.copyWith(terminalMetricColors: {...current.terminalMetricColors, metric: color}),
                          ),
                        );
                      },
                    ),
                  ],
                ),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton(
                  onPressed: local.hiddenTerminalMetrics.isEmpty && local.terminalMetricColors.isEmpty
                      ? null
                      : () => runWithFeedback(
                          context,
                          () => settings.updateLocal(
                            settings.currentLocal.copyWith(hiddenTerminalMetrics: {}, terminalMetricColors: {}),
                          ),
                        ),
                  child: Text(l10n.terminalBarReset),
                ),
              ),
            ],
          ),
        const TerminalAppearanceControls(),
        const TerminalTabPlacementSetting(id: 'terminal-tab-placement'),
        LabeledValue(
          label: l10n.settingsTerminalTabSubtitle,
          value: Align(
            alignment: Alignment.centerLeft,
            child: GlassSelect<TerminalTabSubtitle>(
              key: const ValueKey('terminal-tab-subtitle'),
              value: local.terminalTabSubtitle,
              semanticLabel: l10n.settingsTerminalTabSubtitle,
              items: [
                GlassSelectItem(value: TerminalTabSubtitle.name, label: l10n.settingsTerminalTabSubtitleName),
                GlassSelectItem(value: TerminalTabSubtitle.address, label: l10n.settingsTerminalTabSubtitleAddress),
                GlassSelectItem(value: TerminalTabSubtitle.off, label: l10n.settingsTerminalTabSubtitleOff),
              ],
              onChanged: (v) => settings.updateLocal(settings.currentLocal.copyWith(terminalTabSubtitle: v)),
            ),
          ),
        ),
        LabeledValue(
          label: l10n.settingsScrollbackLabel,
          value: Align(
            alignment: Alignment.centerLeft,
            child: GlassSelect<int>(
              key: const ValueKey('scrollback'),
              value: const [1000, 5000, 10000, 50000].contains(local.terminalScrollback)
                  ? local.terminalScrollback
                  : 5000,
              items: [
                for (final n in const [1000, 5000, 10000, 50000])
                  GlassSelectItem(value: n, label: l10n.settingsScrollbackLines(n)),
              ],
              onChanged: (v) => settings.updateLocal(local.copyWith(terminalScrollback: v)),
            ),
          ),
        ),
        Text(
          l10n.settingsTerminalBuffersNote,
          style: GlassTokens.of(context).typography.callout.copyWith(color: GlassTokens.of(context).secondaryLabel),
        ),
      ],
    );
  }
}

class ThemeAndGlassControls extends ConsumerWidget {
  const ThemeAndGlassControls({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final settings = ref.watch(settingsServiceProvider);
    final local = ref.watch(localSettingsProvider).value ?? settings.currentLocal;
    final tokens = GlassTokens.of(context);
    final strings = GlassStrings.of(context);
    final glass = GlassScope.of(context);
    final solidReason = glass.solidReason;
    final help = tokens.typography.callout.copyWith(color: tokens.secondaryLabel);
    // Why glass renders solid although another option is picked (OS setting,
    // remote session, battery saver, frame guard); nothing when it is the
    // user's own choice.
    final solidNote = switch (solidReason) {
      GlassSolidReason.reduceTransparency => l10n.settingsGlassSolidReduceTransparency,
      GlassSolidReason.remoteSession => l10n.settingsGlassSolidRemote,
      GlassSolidReason.batterySaver => l10n.settingsGlassSolidBattery,
      GlassSolidReason.performance => l10n.settingsGlassSolidPerformance,
      GlassSolidReason.setting || null => null,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        LabeledValue(
          label: l10n.settingsThemeLabel,
          value: Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassSegmented<AppThemeMode>(
              key: const ValueKey('theme-mode'),
              inChrome: false,
              segments: [
                GlassSegment(value: AppThemeMode.system, label: l10n.settingsThemeSystem),
                GlassSegment(value: AppThemeMode.light, label: l10n.settingsThemeLight),
                GlassSegment(value: AppThemeMode.dark, label: l10n.settingsThemeDark),
              ],
              selected: local.themeMode,
              onChanged: (m) => settings.updateLocal(local.copyWith(themeMode: m)),
            ),
          ),
        ),
        LabeledValue(
          label: strings.glassSetting,
          value: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              GlassSegmented<GlassMode>(
                key: const ValueKey('glass-mode'),
                inChrome: false,
                segments: [
                  for (final m in GlassMode.values)
                    GlassSegment(key: ValueKey('glass-mode-${m.wireName}'), value: m, label: strings.glassMode(m)),
                ],
                selected: local.glassMode,
                onChanged: (m) {
                  // Re-selecting the checked segment is also an explicit
                  // retry after automatic performance reduction.
                  glass.retryEffects?.call();
                  runWithFeedback(context, () => settings.updateLocal(settings.currentLocal.copyWith(glassMode: m)));
                },
              ),
              const SizedBox(height: GlassSpacing.s4),
              Text(l10n.settingsGlassHelp, style: help),
              if (solidNote != null)
                Padding(
                  padding: const EdgeInsets.only(top: GlassSpacing.s4),
                  child: Row(
                    children: [
                      Icon(Icons.info_rounded, size: 14, color: tokens.palette.info),
                      const SizedBox(width: GlassSpacing.s4),
                      Expanded(
                        child: Text(
                          solidNote,
                          key: const ValueKey('glass-solid-reason'),
                          style: help.copyWith(color: tokens.palette.info),
                        ),
                      ),
                    ],
                  ),
                ),
              if (solidReason == GlassSolidReason.performance && glass.retryEffects != null)
                GlassButton(
                  key: const ValueKey('glass-retry-effects'),
                  label: l10n.settingsGlassRetryEffects,
                  icon: Icons.refresh_rounded,
                  onPressed: glass.retryEffects,
                ),
            ],
          ),
        ),
      ],
    );
  }
}
