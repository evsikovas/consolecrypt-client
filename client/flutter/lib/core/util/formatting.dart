/// Locale-aware formatting of times, dates and sizes (intl), plus fixed
/// ISO stamps for file names (never localized).
library;

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:intl/intl.dart';

/// "just now", "5 min ago", "3 h ago", "2 d ago", or a date.
String formatRelative(AppLocalizations l10n, DateTime time, {DateTime? now}) {
  final reference = now ?? DateTime.now();
  final diff = reference.difference(time);
  if (diff.isNegative) {
    return formatRemaining(l10n, time, now: reference);
  }
  if (diff.inSeconds < 45) return l10n.timeJustNow;
  if (diff.inMinutes < 60) return l10n.timeMinutesAgo(diff.inMinutes.clamp(1, 59));
  if (diff.inHours < 24) return l10n.timeHoursAgo(diff.inHours);
  if (diff.inDays < 30) return l10n.timeDaysAgo(diff.inDays);
  return formatDate(l10n, time);
}

/// "in 5 min", "in 23 h", "in 3 d" (or "expired").
String formatRemaining(AppLocalizations l10n, DateTime until, {DateTime? now}) {
  final diff = until.difference(now ?? DateTime.now());
  if (diff.isNegative) return l10n.timeExpired;
  if (diff.inMinutes < 1) return l10n.timeInLessThanMinute;
  if (diff.inMinutes < 60) return l10n.timeInMinutes(diff.inMinutes);
  if (diff.inHours < 48) return l10n.timeInHours(diff.inHours);
  return l10n.timeInDays(diff.inDays);
}

/// Medium date in the UI language: "Sep 26, 2026" / "26 сент. 2026 г.".
String formatDate(AppLocalizations l10n, DateTime time) => DateFormat.yMMMd(l10n.localeName).format(time.toLocal());

/// Medium date + 24 h time: "Sep 26, 2026 14:05".
String formatDateTime(AppLocalizations l10n, DateTime time) =>
    DateFormat.yMMMd(l10n.localeName).add_Hm().format(time.toLocal());

/// Human-readable byte size: `512 B`, `1.4 KiB`, `3,2 МиБ`.
String formatBytes(AppLocalizations l10n, int bytes) {
  final units = [l10n.unitBytes, l10n.unitKibibytes, l10n.unitMebibytes, l10n.unitGibibytes, l10n.unitTebibytes];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  final number = unit == 0
      ? NumberFormat.decimalPattern(l10n.localeName).format(bytes)
      : NumberFormat(value >= 100 ? '0' : '0.0', l10n.localeName).format(value);
  return l10n.formatSize(number, units[unit]);
}

/// Plain integer with locale grouping: `12,345` / `12 345`.
String formatCount(AppLocalizations l10n, int value) => NumberFormat.decimalPattern(l10n.localeName).format(value);

/// `2026-09-26` — file names and machine-readable stamps only.
String formatIsoDate(DateTime time) {
  final t = time.toLocal();
  return '${t.year.toString().padLeft(4, '0')}-${_two(t.month)}-${_two(t.day)}';
}

/// `2026-09-26 14:05` — file names and machine-readable stamps only.
String formatIsoDateTime(DateTime time) {
  final t = time.toLocal();
  return '${formatIsoDate(t)} ${_two(t.hour)}:${_two(t.minute)}';
}

String _two(int v) => v.toString().padLeft(2, '0');
