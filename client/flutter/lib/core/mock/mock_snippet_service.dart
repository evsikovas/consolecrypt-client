import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_risk_rules.dart';
import 'package:consolecrypt/core/mock/mock_scope.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

final class MockSnippetService extends VaultScopedMock implements SnippetService {
  MockSnippetService(super.cloud) {
    initScope();
  }

  final ValueStreamController<List<Snippet>> _snippets = ValueStreamController(const []);

  MockConfig get _config => cloud.config;

  /// Tiny "semantic" layer: query words → related vocabulary.
  static const Map<String, List<String>> _synonyms = {
    'disk': ['df', 'du', 'space', 'storage', 'files'],
    'space': ['df', 'du', 'disk'],
    'storage': ['df', 'du', 'disk'],
    'log': ['logs', 'tail', 'journalctl'],
    'logs': ['log', 'tail'],
    'restart': ['systemctl', 'service', 'reload'],
    'service': ['systemctl', 'restart'],
    'port': ['ss', 'listen', 'network', 'socket'],
    'ports': ['ss', 'listen', 'network'],
    'network': ['ss', 'ports'],
    'database': ['postgres', 'sql', 'select', 'table'],
    'db': ['postgres', 'sql', 'table'],
    'query': ['select', 'sql', 'postgres'],
    'pod': ['kubectl', 'k8s'],
    'pods': ['kubectl', 'k8s'],
    'kubernetes': ['kubectl', 'k8s', 'helm'],
    'container': ['docker'],
    'containers': ['docker'],
    'delete': ['rm', 'drop', 'clean', 'cleanup'],
    'clean': ['cleanup', 'delete', 'find'],
    'deploy': ['helm', 'upgrade', 'kubectl'],
    'health': ['cluster', 'status'],
  };

  @override
  void onDataChanged(MockVaultData? data) {
    _snippets.value = List.unmodifiable(data?.snippets ?? const <Snippet>[]);
  }

  @override
  Stream<List<Snippet>> watchSnippets() => _snippets.stream;

  /// Synchronous snapshot (tests / developer tooling).
  List<Snippet> get currentSnippets => _snippets.value;

  @override
  Future<Snippet> saveSnippet(Snippet snippet) async {
    if (snippet.name.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'invalid name: must not be empty',
        reason: AppErrorReason.nameRequired,
      );
    }
    if (snippet.template.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'invalid template: must not be empty',
        reason: AppErrorReason.templateRequired,
      );
    }
    await mockDelay(_config.latency);
    final list = data.snippets;
    final i = list.indexWhere((s) => s.id == snippet.id);
    if (i >= 0) {
      list[i] = snippet;
    } else {
      list.add(snippet);
    }
    onDataChanged(dataOrNull);
    cloud.recordMutation();
    return snippet;
  }

  @override
  Future<void> deleteSnippet(ObjectId id) async {
    await mockDelay(_config.latency);
    data.snippets.removeWhere((s) => s.id == id);
    onDataChanged(dataOrNull);
    cloud.recordMutation();
  }

  @override
  Future<List<SnippetSearchHit>> search(String query, {int limit = 20}) async {
    await mockDelay(Duration.zero);
    final snippets = dataOrNull?.snippets ?? const <Snippet>[];
    final q = query.trim().toLowerCase();
    if (q.isEmpty) {
      final sorted = [...snippets]..sort((a, b) => b.usageCount.compareTo(a.usageCount));
      return [for (final s in sorted.take(limit)) SnippetSearchHit(snippet: s, score: s.usageCount.toDouble())];
    }
    final words = q.split(RegExp(r'\s+'));
    final hits = <SnippetSearchHit>[];
    for (final s in snippets) {
      final name = s.name.toLowerCase();
      final haystack = [
        name,
        s.description,
        s.packageName ?? '',
        s.template,
        ...s.tags,
        s.snippetType.label,
      ].join(' ').toLowerCase();
      var score = 0.0;
      var kind = SearchMatchKind.text;
      if (name == q) {
        score += 100;
        kind = SearchMatchKind.exact;
      } else if (name.contains(q)) {
        score += 40;
      }
      for (final w in words) {
        if (haystack.contains(w)) score += 10;
      }
      if (score == 0) {
        for (final w in words) {
          for (final related in _synonyms[w] ?? const <String>[]) {
            if (haystack.contains(related)) {
              score += 4;
              kind = SearchMatchKind.semantic;
            }
          }
        }
      }
      if (score > 0) {
        hits.add(SnippetSearchHit(snippet: s, score: score + s.usageCount * 0.1, matchKind: kind));
      }
    }
    hits.sort((a, b) => b.score.compareTo(a.score));
    return hits.take(limit).toList();
  }

  @override
  Future<String> render(Snippet snippet, Map<String, String> values) async {
    final missing = [
      for (final v in snippet.effectiveVariables)
        if (v.required && (values[v.name] ?? v.defaultValue ?? '').trim().isEmpty) v.name,
    ];
    if (missing.isNotEmpty) {
      throw AppException(
        AppErrorCode.validation,
        'Fill in: ${missing.join(', ')}',
        reason: AppErrorReason.missingVariables,
        args: {'names': missing.join(', ')},
      );
    }
    final merged = {
      for (final v in snippet.effectiveVariables)
        if ((values[v.name] ?? v.defaultValue) != null) v.name: values[v.name] ?? v.defaultValue!,
    };
    return renderTemplate(snippet.template, merged);
  }

  @override
  Future<RiskAssessment> assessRisk(
    String command, {
    RiskLevel declared = RiskLevel.unknown,
    SnippetSource source = SnippetSource.user,
  }) async {
    await mockDelay(Duration.zero);
    return MockRiskRules.assess(command, declared: declared, source: source);
  }

  @override
  Future<void> recordUsage(ObjectId id) async {
    final list = dataOrNull?.snippets;
    if (list == null) return;
    final i = list.indexWhere((s) => s.id == id);
    if (i < 0) return;
    list[i] = list[i].copyWith(lastUsedAt: DateTime.now().toUtc(), usageCount: list[i].usageCount + 1);
    onDataChanged(dataOrNull);
    cloud.recordMutation();
  }

  Future<void> dispose() async {
    await disposeScope();
    await _snippets.close();
  }
}
