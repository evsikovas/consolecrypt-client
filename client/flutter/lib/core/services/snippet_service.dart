import 'package:consolecrypt/core/models/models.dart';

/// Snippets (synced E2EE) with local search (search-core) and the local
/// risk rules + templating of ai-core.
abstract interface class SnippetService {
  Stream<List<Snippet>> watchSnippets();

  Future<Snippet> saveSnippet(Snippet snippet);

  Future<void> deleteSnippet(ObjectId id);

  /// Exact / FTS first, then semantic (CLIENT_SPEC §12.1). Empty query →
  /// most recently / frequently used.
  Future<List<SnippetSearchHit>> search(String query, {int limit = 20});

  /// Fills `{{variables}}` (quoting policy owned by the core).
  Future<String> render(Snippet snippet, Map<String, String> values);

  /// Local rules decide; [declared] and [source] feed `combineRisk`.
  Future<RiskAssessment> assessRisk(
    String command, {
    RiskLevel declared = RiskLevel.unknown,
    SnippetSource source = SnippetSource.user,
  });

  Future<void> recordUsage(ObjectId id);
}
