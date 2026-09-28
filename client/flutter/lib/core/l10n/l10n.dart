/// Localization entry point for widgets: `context.l10n.someKey`.
///
/// Strings live in `lib/l10n/app_<locale>.arb` (English is the template);
/// `flutter gen-l10n` (run by `flutter pub get` / builds) generates
/// [AppLocalizations]. How to add a language: ADR-0101 "Localization".
library;

import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/widgets.dart';

export 'package:consolecrypt/l10n/app_localizations.dart';

extension AppLocalizationsContext on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);
}

/// Language used when the OS language is not supported.
const fallbackLocale = Locale('en');

/// [MaterialApp.locale] for a stored preference; `null` follows the OS.
Locale? localeFor(AppLocale preference) {
  final code = preference.languageCode;
  return code == null ? null : Locale(code);
}

/// Picks the first OS-preferred language we support, else English.
Locale resolveSystemLocale(List<Locale>? preferred, Iterable<Locale> supported) {
  for (final locale in preferred ?? const <Locale>[]) {
    for (final s in supported) {
      if (s.languageCode == locale.languageCode) return s;
    }
  }
  return fallbackLocale;
}

/// Each language in its own name (never translated), for language pickers.
String appLocaleNativeName(AppLocale locale, AppLocalizations l10n) => switch (locale) {
  AppLocale.system => l10n.languageSystem,
  AppLocale.en => 'English',
  AppLocale.ru => 'Русский',
};
