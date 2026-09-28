import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Stores the UI language (device-local setting). MaterialApp watches it, so
/// the whole UI switches immediately.
Future<void> setAppLocale(WidgetRef ref, AppLocale locale) {
  final settings = ref.read(settingsServiceProvider);
  return settings.updateLocal(settings.currentLocal.copyWith(appLocale: locale));
}

/// "System (English)": the System option names the language it resolves to.
String appLocaleOptionLabel(AppLocale locale, AppLocalizations l10n) {
  if (locale != AppLocale.system) return appLocaleNativeName(locale, l10n);
  final resolved = resolveSystemLocale(
    WidgetsBinding.instance.platformDispatcher.locales,
    AppLocalizations.supportedLocales,
  );
  final name = appLocaleNativeName(AppLocale.fromWire(resolved.languageCode), l10n);
  return l10n.languageSystemResolved(name);
}

/// Language selector for Settings → Appearance (a [GlassSelect]).
class LanguageDropdown extends ConsumerWidget {
  const LanguageDropdown({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = ref.watch(localSettingsProvider).value?.appLocale ?? AppLocale.system;
    final l10n = context.l10n;
    return GlassSelect<AppLocale>(
      key: const ValueKey('app-locale'),
      value: current,
      semanticLabel: l10n.languageLabel,
      items: [
        for (final locale in AppLocale.values)
          GlassSelectItem(
            key: ValueKey('app-locale-${locale.wireName}'),
            value: locale,
            label: appLocaleOptionLabel(locale, l10n),
          ),
      ],
      onChanged: (v) => unawaited(setAppLocale(ref, v)),
    );
  }
}

/// Compact switcher for the first-launch screen (top-right, in the title-bar
/// band), so a user can pick a language before creating a profile: a glass
/// capsule that opens a glass menu.
class LanguageMenuButton extends ConsumerWidget {
  const LanguageMenuButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = ref.watch(localSettingsProvider).value?.appLocale ?? AppLocale.system;
    final l10n = context.l10n;
    final label = current == AppLocale.system
        ? appLocaleNativeName(AppLocale.fromWire(Localizations.localeOf(context).languageCode), l10n)
        : appLocaleNativeName(current, l10n);
    return GlassMenuButton<AppLocale>(
      entries: [
        for (final locale in AppLocale.values)
          GlassMenuItem(
            key: ValueKey('language-${locale.wireName}'),
            value: locale,
            label: appLocaleOptionLabel(locale, l10n),
            checked: locale == current,
          ),
      ],
      onSelected: (v) => unawaited(setAppLocale(ref, v)),
      builder: (context, open) => GlassButton(
        key: const ValueKey('language-menu'),
        icon: Icons.translate_rounded,
        label: label,
        tooltip: l10n.languageSwitcherTooltip,
        onPressed: open,
      ),
    );
  }
}
