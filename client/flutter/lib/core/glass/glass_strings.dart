import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/widgets.dart';

/// User-facing strings of the glass kit, read from the app's ARB files
/// (`glass*` keys plus a few `common*` / `risk*` keys).
///
/// Outside a localized app (isolated widget tests, the standalone gallery
/// without delegates) it falls back to English so the kit never throws.
final class GlassStrings {
  const GlassStrings(this._l);

  /// Strings for the locale of [context].
  factory GlassStrings.of(BuildContext context) =>
      GlassStrings(Localizations.of<AppLocalizations>(context, AppLocalizations) ?? _english);

  static final AppLocalizations _english = lookupAppLocalizations(const Locale('en'));

  final AppLocalizations _l;

  String glassMode(GlassMode mode) => switch (mode) {
    GlassMode.clear => _l.glassModeClear,
    GlassMode.standard => _l.glassModeStandard,
    GlassMode.tinted => _l.glassModeTinted,
    GlassMode.solid => _l.glassModeSolid,
  };

  String get glassSetting => _l.glassSetting;
  String get show => _l.commonShow;
  String get hide => _l.commonHide;
  String get capsLockOn => _l.glassCapsLockOn;
  String get dismiss => _l.glassDismiss;
  String get closeTab => _l.glassCloseTab;
  String get newTab => _l.glassNewTab;
  String get scrollTabsLeft => _l.glassScrollTabsLeft;
  String get scrollTabsRight => _l.glassScrollTabsRight;
  String get tabConnected => _l.glassTabConnected;
  String get tabReconnecting => _l.glassTabReconnecting;
  String get tabDisconnected => _l.glassTabDisconnected;
  String get verificationCode => _l.glassVerificationCode;
  String get moreActions => _l.glassMoreActions;

  /// "Group 1: 4 8 2 1 3" — digits spelled out for screen readers.
  String codeGroup(int number, String digits) => _l.glassCodeGroup(number, digits.split('').join(' '));

  String clearsIn(int seconds) => _l.glassClearsIn(seconds);

  /// "Risk: Destructive".
  String risk(String level) => _l.riskSemantics(level);
}
