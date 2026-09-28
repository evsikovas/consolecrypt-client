import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/snippets/starter_catalog.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final en = lookupAppLocalizations(const Locale('en'));
  final ru = lookupAppLocalizations(const Locale('ru'));
  test('three editable starter packages contain 15 read-only commands and complete variables', () {
    final packages = starterSnippetPackages(en);
    expect(packages.map((p) => p.id), ['linux', 'docker', 'kubernetes']);
    final snippets = packages.expand((p) => p.snippets).toList();
    expect(snippets, hasLength(15));
    expect(snippets.map((s) => s.catalogId).toSet(), hasLength(15));
    for (final s in snippets) {
      expect(s.packageName, isNotEmpty);
      expect(s.riskLevel, RiskLevel.readOnly);
      expect(s.source, SnippetSource.imported);
      expect(s.variables.map((v) => v.name).toSet(), templateVariables(s.template).toSet());
      final copy = snippetFromJson(snippetToJson(s));
      expect(copy.packageName, s.packageName);
      expect(copy.catalogId, s.catalogId);
      expect(copy.template, s.template);
    }
  });
  test('reimport skips edited entries across locale and package renames, and can restore a deletion', () {
    final english = starterSnippetPackages(en).first;
    final saved = english.snippets
        .map((s) => s.copyWith(name: 'Custom', template: 'df -h /', packageName: 'Mine'))
        .toList();
    final russian = starterSnippetPackages(ru).first;
    expect(russian.missingFrom(saved), isEmpty);
    final removed = saved.removeAt(0);
    expect(russian.missingFrom(saved).single.catalogId, removed.catalogId);
    final edited = saved.first.copyWith(usageCount: 5, clearPackage: true);
    expect(edited.packageName, isNull);
    expect(edited.catalogId, saved.first.catalogId);
    expect(edited.template, saved.first.template);
  });
}
