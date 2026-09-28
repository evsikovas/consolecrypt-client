import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/passphrase_strength.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';

Map<String, dynamic> _arb(String locale) =>
    jsonDecode(File('lib/l10n/app_$locale.arb').readAsStringSync()) as Map<String, dynamic>;

Iterable<String> _messageKeys(Map<String, dynamic> arb) => arb.keys.where((k) => !k.startsWith('@'));

/// Top-level `{name}` / `{name, plural, …}` arguments of an ICU message.
Set<String> _arguments(String message) {
  final names = <String>{};
  for (final m in RegExp(r'\{\s*(\w+)\s*[,}]').allMatches(message)) {
    names.add(m.group(1)!);
  }
  return names;
}

/// Selectors of every `plural` in [message] (e.g. {one, few, many, other}).
List<Set<String>> _pluralSelectors(String message) {
  final result = <Set<String>>[];
  for (final m in RegExp(r'\{\s*\w+\s*,\s*plural\s*,').allMatches(message)) {
    var depth = 1;
    var i = m.end;
    final selectors = <String>{};
    final current = StringBuffer();
    while (i < message.length && depth > 0) {
      final c = message[i];
      if (c == '{') {
        if (depth == 1) {
          selectors.add(current.toString().trim());
          current.clear();
        }
        depth++;
      } else if (c == '}') {
        depth--;
      } else if (depth == 1) {
        current.write(c);
      }
      i++;
    }
    result.add(selectors);
  }
  return result;
}

void main() {
  final en = _arb('en');
  final ru = _arb('ru');
  final l10nEn = lookupAppLocalizations(const Locale('en'));
  final l10nRu = lookupAppLocalizations(const Locale('ru'));

  group('ARB files', () {
    test('every key exists in both languages', () {
      final enKeys = _messageKeys(en).toSet();
      final ruKeys = _messageKeys(ru).toSet();
      expect(enKeys.difference(ruKeys), isEmpty, reason: 'missing in app_ru.arb');
      expect(ruKeys.difference(enKeys), isEmpty, reason: 'only in app_ru.arb');
      expect(enKeys.length, greaterThan(500));
    });

    test('every key has a description; placeholders match the message in both languages', () {
      for (final key in _messageKeys(en)) {
        final meta = en['@$key'] as Map<String, dynamic>?;
        expect(meta?['description'], isA<String>().having((d) => d.isNotEmpty, 'non-empty', isTrue), reason: key);
        final declared = ((meta?['placeholders'] as Map<String, dynamic>?) ?? const {}).keys.toSet();
        expect(_arguments(en[key] as String), declared, reason: 'en $key');
        expect(_arguments(ru[key] as String), declared, reason: 'ru $key');
      }
    });

    test('Russian plurals define one, few, many and other (and never =1)', () {
      for (final key in _messageKeys(ru)) {
        final message = ru[key] as String;
        for (final selectors in _pluralSelectors(message)) {
          expect(selectors, containsAll(['one', 'few', 'many', 'other']), reason: key);
        }
        expect(message.contains(RegExp(r'=1\s*\{')), isFalse, reason: '$key: =1 overrides "one" (21, 31…)');
      }
      for (final key in _messageKeys(en)) {
        for (final selectors in _pluralSelectors(en[key] as String)) {
          expect(selectors, contains('other'), reason: key);
        }
      }
    });

    test('Russian is actually translated (not a copy of the English template)', () {
      final same = _messageKeys(en).where((k) => en[k] == ru[k] && RegExp('[a-z]{4,}').hasMatch(en[k] as String));
      // Technical/brand-only strings may legitimately be identical.
      expect(same.length, lessThan(40), reason: same.join(', '));
    });
  });

  group('Russian plural forms', () {
    test('1, 2, 5, 21 items', () {
      expect(l10nRu.timeDaysAgo(1), '1 день назад');
      expect(l10nRu.timeDaysAgo(2), '2 дня назад');
      expect(l10nRu.timeDaysAgo(5), '5 дней назад');
      expect(l10nRu.timeDaysAgo(21), '21 день назад');
      expect(l10nRu.timeInDays(3), 'через 3 дня');
      expect(l10nRu.timeInDays(11), 'через 11 дней');
      expect(l10nRu.shellApprovalBannerTitle(1), '1 запрос на подтверждение устройства');
      expect(l10nRu.shellApprovalBannerTitle(2), '2 запроса на подтверждение устройств');
      expect(l10nRu.shellApprovalBannerTitle(5), '5 запросов на подтверждение устройств');
      expect(l10nRu.shellApprovalBannerTitle(21), '21 запрос на подтверждение устройства');
      expect(l10nRu.syncIndicatorPendingTooltip('Офлайн', 1), 'Офлайн — 1 локальное изменение ожидает отправки');
      expect(l10nRu.syncIndicatorPendingTooltip('Офлайн', 3), 'Офлайн — 3 локальных изменения ожидают отправки');
      expect(l10nRu.syncIndicatorPendingTooltip('Офлайн', 25), 'Офлайн — 25 локальных изменений ожидают отправки');
      expect(l10nRu.syncIndicatorPendingTooltip('Офлайн', 21), 'Офлайн — 21 локальное изменение ожидает отправки');
    });

    test('English uses singular/plural', () {
      expect(l10nEn.shellApprovalBannerTitle(1), 'Device approval requested');
      expect(l10nEn.shellApprovalBannerTitle(3), '3 device approval requests');
      expect(l10nEn.syncIndicatorPendingTooltip('Offline', 1), 'Offline — 1 local change waiting');
    });
  });

  group('locale-aware formatting', () {
    setUpAll(() async {
      await initializeDateFormatting('en');
      await initializeDateFormatting('ru');
    });

    final now = DateTime(2026, 9, 26, 12);

    test('relative and remaining time', () {
      expect(formatRelative(l10nEn, now.subtract(const Duration(seconds: 10)), now: now), 'just now');
      expect(formatRelative(l10nEn, now.subtract(const Duration(minutes: 5)), now: now), '5 min ago');
      expect(formatRelative(l10nRu, now.subtract(const Duration(minutes: 5)), now: now), '5 мин назад');
      expect(formatRelative(l10nRu, now.subtract(const Duration(hours: 3)), now: now), '3 ч назад');
      expect(formatRelative(l10nRu, now.subtract(const Duration(days: 2)), now: now), '2 дня назад');
      expect(formatRemaining(l10nEn, now.add(const Duration(minutes: 5)), now: now), 'in 5 min');
      expect(formatRemaining(l10nRu, now.add(const Duration(hours: 5)), now: now), 'через 5 ч');
      expect(formatRemaining(l10nRu, now.subtract(const Duration(hours: 1)), now: now), 'срок истёк');
    });

    test('dates follow the UI language', () {
      final t = DateTime(2026, 9, 26, 14, 5);
      expect(formatDate(l10nEn, t), 'Sep 26, 2026');
      expect(formatDate(l10nRu, t), contains('2026'));
      expect(formatDate(l10nRu, t), contains('сент'));
      expect(formatDateTime(l10nRu, t), endsWith('14:05'));
      expect(formatIsoDate(t), '2026-09-26');
    });

    test('byte sizes use localized units and decimal separators', () {
      expect(formatBytes(l10nEn, 512), '512 B');
      expect(formatBytes(l10nEn, 1536), '1.5 KiB');
      expect(formatBytes(l10nRu, 1536), '1,5 КиБ');
      expect(formatBytes(l10nRu, 3 * 1024 * 1024), '3,0 МиБ');
      expect(formatBytes(l10nRu, 200 * 1024), '200 КиБ');
    });
  });

  group('errors', () {
    test('every code and reason has localized text; raw English is never shown', () {
      for (final code in AppErrorCode.values) {
        final e = AppException(code, 'RAW ENGLISH DIAGNOSTIC');
        for (final l in [l10nEn, l10nRu]) {
          final text = errorMessage(l, e);
          expect(text, isNotEmpty, reason: code.name);
          expect(text, isNot(contains('RAW ENGLISH')), reason: code.name);
        }
        expect(errorMessage(l10nRu, e), isNot(errorMessage(l10nEn, e)), reason: code.name);
      }
      for (final reason in AppErrorReason.values) {
        final e = AppException(
          AppErrorCode.validation,
          'RAW',
          reason: reason,
          args: const {'path': '/srv', 'name': 'x', 'names': 'a, b', 'field': 'port', 'rule': 'must be 1..=65535'},
        );
        expect(errorMessage(l10nRu, e), isNot(errorMessage(l10nEn, e)), reason: reason.name);
        expect(AppErrorReason.fromWire(reason.wireName), reason);
      }
    });

    test('model validation errors are localized', () {
      const e = ValidationError('port', 'must be 1..=65535');
      expect(errorMessage(l10nEn, e), 'Port: must be 1–65535');
      expect(errorMessage(l10nRu, e), 'Порт: допустимо 1–65535');
      expect(
        errorMessage(l10nRu, AppException.fromValidation(const ValidationError('name', 'must not be empty'))),
        'Имя: обязательное поле',
      );
    });

    test('rate limiting shows the retry delay', () {
      const e = AppException(AppErrorCode.rateLimited, 'x', args: {'retry_after_seconds': '30'});
      expect(errorMessage(l10nRu, e), 'Слишком много попыток. Повторите через 30 с.');
    });
  });

  group('labels', () {
    test('enum labels are translated, technical tokens are not', () {
      expect(RiskLevel.destructive.localized(l10nEn), 'Destructive');
      expect(RiskLevel.destructive.localized(l10nRu), 'Разрушительная');
      expect(TunnelKind.dynamic.localized(l10nRu), 'Динамический (SOCKS5)');
      expect(KeyAlgorithm.ed25519.localized(l10nRu), 'Ed25519');
      expect(SnippetType.powershell.localized(l10nRu), 'PowerShell');
      expect(CredentialKind.sshPrivateKey.localized(l10nRu), 'SSH-ключ');
      expect(PassphraseStrength.estimate('').localizedHint(l10nRu), contains('12'));
    });
  });

  group('locale resolution', () {
    const supported = AppLocalizations.supportedLocales;

    test('System follows the first supported OS language, else English', () {
      expect(resolveSystemLocale(const [Locale('ru', 'RU')], supported), const Locale('ru'));
      expect(resolveSystemLocale(const [Locale('de'), Locale('ru')], supported), const Locale('ru'));
      expect(resolveSystemLocale(const [Locale('de', 'DE')], supported), const Locale('en'));
      expect(resolveSystemLocale(null, supported), const Locale('en'));
    });

    test('stored preference maps to a locale (system → null)', () {
      expect(localeFor(AppLocale.system), isNull);
      expect(localeFor(AppLocale.ru), const Locale('ru'));
      expect(AppLocale.fromWire('ru'), AppLocale.ru);
      expect(AppLocale.fromWire('xx'), AppLocale.system);
      expect(const LocalSettings().appLocale, AppLocale.system);
    });
  });
}
