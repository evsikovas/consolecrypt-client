import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_models::snippet::SnippetType` (CLIENT_SPEC §11.1).
enum SnippetType {
  shell('shell', 'Shell'),
  bash('bash', 'Bash'),
  zsh('zsh', 'Zsh'),
  powershell('powershell', 'PowerShell'),
  cmd('cmd', 'cmd'),
  sql('sql', 'SQL'),
  postgresql('postgresql', 'PostgreSQL'),
  kubectl('kubectl', 'kubectl'),
  helm('helm', 'Helm'),
  docker('docker', 'Docker'),
  terraform('terraform', 'Terraform'),
  ansible('ansible', 'Ansible'),
  redisCli('redis_cli', 'Redis CLI'),
  cql('cql', 'CQL'),
  opensearchDsl('opensearch_dsl', 'OpenSearch DSL'),
  httpCurl('http_curl', 'HTTP/curl');

  const SnippetType(this.wireName, this.label);

  final String wireName;
  final String label;
}

/// Mirrors `cc_models::snippet::RiskLevel`. Local rules decide the effective
/// risk; an AI suggestion is only a hint (CLIENT_SPEC §11.3).
enum RiskLevel {
  readOnly('read_only', 0),
  modifying('modifying', 2),
  destructive('destructive', 3),
  unknown('unknown', 1);

  const RiskLevel(this.wireName, this.severity);

  final String wireName;

  /// Ordering used when combining risks: read-only < unknown < modifying <
  /// destructive.
  final int severity;

  /// Mirrors `RiskLevel::requires_confirmation`.
  bool get requiresConfirmation => this != RiskLevel.readOnly;

  RiskLevel atLeast(RiskLevel other) => other.severity > severity ? other : this;
}

/// Mirrors `cc_models::snippet::SnippetSource`.
enum SnippetSource {
  user('user'),
  ai('ai'),
  imported('imported'),
  history('history');

  const SnippetSource(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::snippet::SnippetVariable`.
final class SnippetVariable {
  const SnippetVariable({required this.name, this.description = '', this.defaultValue, this.required = true});

  final String name;
  final String description;
  final String? defaultValue;
  final bool required;
}

/// Mirrors `cc_models::snippet::Snippet`.
final class Snippet {
  const Snippet({
    required this.id,
    required this.name,
    required this.snippetType,
    required this.template,
    required this.createdAt,
    required this.updatedAt,
    this.description = '',
    this.packageName,
    this.catalogId,
    this.shell,
    this.variables = const [],
    this.tags = const [],
    this.riskLevel = RiskLevel.unknown,
    this.source = SnippetSource.user,
    this.createdBy,
    this.lastUsedAt,
    this.usageCount = 0,
  });

  final ObjectId id;
  final String name;
  final String description;
  final String? packageName;
  final String? catalogId;
  final SnippetType snippetType;

  /// Target shell / dialect, free-form (e.g. "bash", "pwsh7").
  final String? shell;
  final String template;
  final List<SnippetVariable> variables;
  final List<String> tags;
  final RiskLevel riskLevel;
  final SnippetSource source;
  final DeviceId? createdBy;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? lastUsedAt;
  final int usageCount;

  /// Variables in template order, merged with declared metadata.
  List<SnippetVariable> get effectiveVariables {
    final declared = {for (final v in variables) v.name: v};
    return [for (final name in templateVariables(template)) declared[name] ?? SnippetVariable(name: name)];
  }

  Snippet copyWith({
    DateTime? lastUsedAt,
    int? usageCount,
    String? name,
    String? template,
    String? packageName,
    bool clearPackage = false,
    DateTime? updatedAt,
  }) => Snippet(
    id: id,
    name: name ?? this.name,
    description: description,
    packageName: clearPackage ? null : packageName ?? this.packageName,
    catalogId: catalogId,
    snippetType: snippetType,
    shell: shell,
    template: template ?? this.template,
    variables: variables,
    tags: tags,
    riskLevel: riskLevel,
    source: source,
    createdBy: createdBy,
    createdAt: createdAt,
    updatedAt: updatedAt ?? this.updatedAt,
    lastUsedAt: lastUsedAt ?? this.lastUsedAt,
    usageCount: usageCount ?? this.usageCount,
  );
}

bool _isValidVariableName(String name) {
  if (name.isEmpty) return false;
  final first = name.codeUnitAt(0);
  final firstOk = _isAsciiAlpha(first) || first == 0x5f; // _
  return firstOk &&
      name.codeUnits.every(
        (c) =>
            _isAsciiAlpha(c) ||
            (c >= 0x30 && c <= 0x39) ||
            c == 0x5f || // _
            c == 0x2e || // .
            c == 0x2d, // -
      );
}

bool _isAsciiAlpha(int c) => (c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a);

/// Mirrors `cc_models::snippet::template_variables`: names of `{{variable}}`
/// placeholders in order of first appearance.
List<String> templateVariables(String template) {
  final out = <String>[];
  var rest = template;
  while (true) {
    final start = rest.indexOf('{{');
    if (start < 0) break;
    final after = rest.substring(start + 2);
    final end = after.indexOf('}}');
    if (end < 0) break;
    final name = after.substring(0, end).trim();
    if (_isValidVariableName(name) && !out.contains(name)) {
      out.add(name);
    }
    rest = after.substring(end + 2);
  }
  return out;
}

/// Substitutes `{{name}}` placeholders. Unknown / invalid placeholders are
/// left untouched. Quoting policy is owned by ai-core's snippet engine; this
/// is the plain substitution used for previews and by the mock backend.
String renderTemplate(String template, Map<String, String> values) {
  return template.replaceAllMapped(RegExp(r'\{\{([^{}]*)\}\}'), (m) {
    final name = m.group(1)!.trim();
    if (_isValidVariableName(name) && values.containsKey(name)) {
      return values[name]!;
    }
    return m.group(0)!;
  });
}

/// Combines a declared risk with the local-rules classification.
///
/// * The more severe of the two always wins when local rules recognise the
///   command as modifying/destructive.
/// * A user-declared level is trusted when local rules don't know the
///   command; an AI-declared level is not (it becomes at least `unknown`).
RiskLevel combineRisk({required RiskLevel declared, required RiskLevel local, required SnippetSource source}) {
  if (local == RiskLevel.unknown) {
    return source == SnippetSource.ai ? declared.atLeast(RiskLevel.unknown) : declared;
  }
  return declared.atLeast(local);
}

/// Output of `SnippetService.assessRisk` (ai-core local rules).
final class RiskAssessment {
  const RiskAssessment({required this.effective, required this.local, required this.declared, this.reasons = const []});

  final RiskLevel effective;
  final RiskLevel local;
  final RiskLevel declared;

  /// Human-readable matches, e.g. "`rm -rf` deletes files recursively".
  final List<String> reasons;
}

/// A search result from the local knowledge base (FTS + semantic).
final class SnippetSearchHit {
  const SnippetSearchHit({required this.snippet, required this.score, this.matchKind = SearchMatchKind.text});

  final Snippet snippet;
  final double score;
  final SearchMatchKind matchKind;
}

enum SearchMatchKind { exact, text, semantic }
