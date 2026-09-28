import 'dart:async';

import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_scope.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/value_stream.dart';

/// Canned, keyword-driven "LLM". Demonstrates streaming, the sanitizer
/// report and — deliberately — AI risk hints that local rules override
/// (e.g. it calls `find … -delete` read-only).
final class MockAiService extends VaultScopedMock implements AiService {
  MockAiService(super.cloud) {
    initScope();
  }

  final ValueStreamController<List<AiProviderConfig>> _providers = ValueStreamController(const []);

  MockConfig get _config => cloud.config;

  @override
  void onDataChanged(MockVaultData? data) {
    _providers.value = List.unmodifiable(data?.providers ?? const <AiProviderConfig>[]);
  }

  @override
  Stream<List<AiProviderConfig>> watchProviders() => _providers.stream;

  @override
  Future<AiProviderConfig> saveProvider(AiProviderConfig config, {SecretText? apiKey, bool clearApiKey = false}) async {
    final url = Uri.tryParse(config.baseUrl.trim());
    if (config.name.trim().isEmpty) {
      throw const AppException(AppErrorCode.validation, 'Enter a name', reason: AppErrorReason.nameRequired);
    }
    if (url == null || !url.hasScheme || url.host.isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter a valid base URL',
        reason: AppErrorReason.invalidBaseUrl,
      );
    }
    if (config.chatModel.trim().isEmpty) {
      throw const AppException(
        AppErrorCode.validation,
        'Enter the chat model name',
        reason: AppErrorReason.chatModelRequired,
      );
    }
    await mockDelay(_config.latency);
    var saved = config;
    if (clearApiKey) {
      saved = saved.copyWith(clearApiKey: true);
    } else if (apiKey != null && apiKey.isNotEmpty) {
      // Stored as a separate Secret object; only its id comes back.
      saved = saved.copyWith(apiKeySecretId: ObjectId.generate());
      apiKey.wipe();
    }
    final list = data.providers;
    if (saved.isDefault) {
      for (var i = 0; i < list.length; i++) {
        if (list[i].isDefault && list[i].id != saved.id) list[i] = list[i].copyWith(isDefault: false);
      }
    }
    final i = list.indexWhere((p) => p.id == saved.id);
    if (i >= 0) {
      list[i] = saved;
    } else {
      list.add(saved);
    }
    onDataChanged(dataOrNull);
    cloud.recordMutation();
    return saved;
  }

  @override
  Future<void> deleteProvider(ObjectId id) async {
    await mockDelay(_config.latency);
    data.providers.removeWhere((p) => p.id == id);
    onDataChanged(dataOrNull);
    cloud.recordMutation();
  }

  AiProviderConfig _provider(ObjectId id) {
    final p = dataOrNull?.providers.where((p) => p.id == id).firstOrNull;
    if (p == null) {
      throw const AppException(AppErrorCode.notFound, 'AI provider not found', reason: AppErrorReason.providerNotFound);
    }
    return p;
  }

  @override
  Future<ProviderHealth> testProvider(ObjectId id) async {
    final p = _provider(id);
    await mockDelay(_config.latency * 2);
    if (p.provider.usuallyNeedsApiKey && !p.hasApiKey) {
      return ProviderHealth(ok: false, message: '${p.provider.label}: 401 Unauthorized — add an API key');
    }
    return ProviderHealth(
      ok: true,
      message: 'Connected to ${Uri.tryParse(p.baseUrl)?.host ?? p.baseUrl}',
      models: [p.chatModel, if (p.embeddingModel != null) p.embeddingModel!, 'nomic-embed-text'],
    );
  }

  SanitizationReport _report(AiProviderConfig p, String text) {
    final secrets = RegExp(
      r'(password|passwd|token|secret|api[_-]?key)\s*[=:]\s*\S+',
      caseSensitive: false,
    ).allMatches(text).length;
    final meta = p.privacyProfile == PrivacyProfile.strict
        ? RegExp(r'\b\d{1,3}(\.\d{1,3}){3}\b|\b[\w-]+\.(example|internal|local|net|org|com)\b').allMatches(text).length
        : 0;
    return SanitizationReport(profile: p.privacyProfile, redactions: secrets + meta, remote: p.isRemote);
  }

  Stream<AiStreamEvent> _stream(ObjectId providerId, String prompt, String answer, {GeneratedCommand? command}) async* {
    final AiProviderConfig p;
    try {
      p = _provider(providerId);
    } on AppException catch (e) {
      yield AiFailed(e.message);
      return;
    }
    if (p.provider.usuallyNeedsApiKey && !p.hasApiKey) {
      yield AiFailed('${p.name}: API key missing. Add it in Settings → AI providers.');
      return;
    }
    await mockDelay(_config.latency);
    final tokens = RegExp(r'\S+\s*|\s+').allMatches(answer).map((m) => m.group(0)!);
    final buffer = StringBuffer();
    for (final t in tokens) {
      await mockDelay(_config.streamStep);
      buffer.write(t);
      yield AiDelta(t);
    }
    yield AiCompleted(fullText: buffer.toString(), report: _report(p, prompt), command: command);
  }

  @override
  Stream<AiStreamEvent> chat({
    required ObjectId providerId,
    required List<ChatMessage> history,
    AiContextSelection context = AiContextSelection.none,
  }) {
    final question = history
        .lastWhere((m) => m.role == ChatRole.user, orElse: () => ChatMessage.now(ChatRole.user, ''))
        .content;
    final suggestion = _suggest(question);
    final answer = StringBuffer()
      ..writeln(suggestion.intro)
      ..writeln()
      ..writeln('```${suggestion.fence}')
      ..writeln(suggestion.command.command)
      ..writeln('```')
      ..writeln()
      ..write(suggestion.command.explanation);
    if (context.selectedTerminalText != null) {
      answer.write(
        '\n\n(I looked at the ${context.selectedTerminalText!.length} characters of terminal output you selected.)',
      );
    }
    return _stream(providerId, question + (context.selectedTerminalText ?? ''), answer.toString());
  }

  @override
  Stream<AiStreamEvent> generateCommand({
    required ObjectId providerId,
    required String request,
    SnippetType? dialect,
    AiContextSelection context = AiContextSelection.none,
  }) {
    final suggestion = _suggest(request);
    return _stream(providerId, request, suggestion.command.command, command: suggestion.command);
  }

  @override
  Future<SnippetDraft> draftSnippet({required String command, ObjectId? providerId}) async {
    await mockDelay(_config.latency);
    var template = command.trim();
    final variables = <SnippetVariable>[];
    void param(RegExp re, String name, String Function(Match) replace) {
      final m = re.firstMatch(template);
      if (m == null) return;
      variables.add(SnippetVariable(name: name, defaultValue: m.group(1)));
      template = template.replaceFirst(re, replace(m));
    }

    param(RegExp(r'-n\s+([\w-]+)'), 'namespace', (_) => '-n {{namespace}}');
    param(RegExp(r'--tail=(\d+)'), 'lines', (_) => '--tail={{lines}}');
    param(RegExp(r'\b(\d{1,3}(?:\.\d{1,3}){3})\b'), 'host', (_) => '{{host}}');
    param(RegExp(r'(/var/log\S*)'), 'path', (_) => '{{path}}');
    param(
      RegExp(r'systemctl\s+\w+\s+([\w@.-]+)'),
      'service',
      (m) => m.group(0)!.replaceFirst(m.group(1)!, '{{service}}'),
    );

    final first = template.split(RegExp(r'\s+')).first.toLowerCase();
    final type = switch (first) {
      'kubectl' => SnippetType.kubectl,
      'helm' => SnippetType.helm,
      'docker' => SnippetType.docker,
      'terraform' => SnippetType.terraform,
      'curl' => SnippetType.httpCurl,
      'select' || 'insert' || 'update' || 'delete' => SnippetType.sql,
      'get' || 'put' || 'post' => SnippetType.opensearchDsl,
      _ => SnippetType.bash,
    };
    final words = command.trim().split(RegExp(r'\s+'));
    return SnippetDraft(
      name: words.take(3).join(' '),
      description: 'Created from an AI suggestion',
      template: template,
      snippetType: type,
      variables: variables,
      tags: ['ai'],
    );
  }

  static _Suggestion _suggest(String request) {
    final q = request.toLowerCase();
    bool has(List<String> words) => words.any(q.contains);
    if (has(['delete', 'remove', 'clean']) && has(['log', 'old'])) {
      return const _Suggestion(
        'This finds compressed logs older than 30 days and deletes them:',
        'bash',
        GeneratedCommand(
          command: "find /var/log -name '*.gz' -mtime +30 -delete",
          explanation: 'Run without `-delete` first to preview the files.',
          suggestedRisk: RiskLevel.readOnly, // wrong on purpose: local rules win
          dialect: SnippetType.bash,
        ),
      );
    }
    if (has(['largest', 'biggest', 'large files'])) {
      return const _Suggestion(
        'List the 20 largest files and directories:',
        'bash',
        GeneratedCommand(
          command: 'du -ah /var/log | sort -rh | head -n 20',
          explanation: '`du -a` includes files; `sort -rh` sorts human-readable sizes descending.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.bash,
        ),
      );
    }
    if (has(['disk', 'space', 'storage'])) {
      return const _Suggestion(
        'Check free disk space per filesystem:',
        'bash',
        GeneratedCommand(
          command: 'df -h',
          explanation: '`-h` prints sizes in human-readable units.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.bash,
        ),
      );
    }
    if (has(['restart', 'reload']) && has(['nginx', 'service', 'web'])) {
      return const _Suggestion(
        'Restart nginx with systemd:',
        'bash',
        GeneratedCommand(
          command: 'sudo systemctl restart nginx',
          explanation: 'Consider `nginx -t` first to validate the configuration.',
          suggestedRisk: RiskLevel.modifying,
          dialect: SnippetType.bash,
        ),
      );
    }
    if (has(['pod', 'kubernetes', 'k8s']) && has(['log'])) {
      return const _Suggestion(
        'Tail the logs of the api deployment:',
        'bash',
        GeneratedCommand(
          command: 'kubectl logs -n production deploy/api --tail=200',
          explanation: 'Add `-f` to follow the log stream.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.kubectl,
        ),
      );
    }
    if (has(['pod', 'kubernetes', 'k8s'])) {
      return const _Suggestion(
        'List pods in all namespaces:',
        'bash',
        GeneratedCommand(
          command: 'kubectl get pods -A -o wide',
          explanation: '`-o wide` adds node and IP columns.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.kubectl,
        ),
      );
    }
    if (has(['port', 'listen', 'socket'])) {
      return const _Suggestion(
        'Show listening TCP/UDP sockets with their processes:',
        'bash',
        GeneratedCommand(
          command: 'ss -tulpn',
          explanation: 'Needs root to show process names of other users.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.bash,
        ),
      );
    }
    if (has(['postgres', 'query', 'queries', 'sql'])) {
      return const _Suggestion(
        'Active queries in PostgreSQL:',
        'sql',
        GeneratedCommand(
          command: "SELECT pid, usename, state, query FROM pg_stat_activity WHERE state <> 'idle';",
          explanation: 'Use `pg_cancel_backend(pid)` to cancel a query.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.postgresql,
        ),
      );
    }
    if (has(['memory', 'ram'])) {
      return const _Suggestion(
        'Show memory usage:',
        'bash',
        GeneratedCommand(
          command: 'free -h',
          explanation: '"available" is what new processes can use.',
          suggestedRisk: RiskLevel.readOnly,
          dialect: SnippetType.bash,
        ),
      );
    }
    return _Suggestion(
      'I am the mock model, so here is a safe starting point for "${request.trim()}":',
      'bash',
      const GeneratedCommand(
        command: 'ls -la',
        explanation: 'Connect a real provider (Ollama, LM Studio, DeepSeek) for useful answers.',
        suggestedRisk: RiskLevel.readOnly,
        dialect: SnippetType.bash,
      ),
    );
  }

  Future<void> dispose() async {
    await disposeScope();
    await _providers.close();
  }
}

final class _Suggestion {
  const _Suggestion(this.intro, this.fence, this.command);

  final String intro;
  final String fence;
  final GeneratedCommand command;
}
