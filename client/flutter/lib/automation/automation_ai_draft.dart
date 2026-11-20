import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

const automationRequirementsFile = '.consolecrypt-requirements.json';

enum AutomationAiDraftError {
  invalidProposal,
  invalidRequirements,
  dependencyConflict,
  limitExceeded,
  invalidPath,
  canonicalFilename,
}

/// Code-only diagnostics: never carry model output or manually edited file text.
/// FormatException compatibility preserves existing validation catch boundaries.
final class AutomationAiDraftException extends FormatException {
  const AutomationAiDraftException(this.code) : super('automation_ai_draft');
  final AutomationAiDraftError code;
}

/// A document proposal has no file/execution capability. Saving the playbook
/// and these independently reviewed constraints remains a separate UI action.
final class AutomationAiDraft {
  factory AutomationAiDraft(AutomationJson proposal) {
    final files = proposal['files'], variables = proposal['variables'], dependencies = proposal['dependencies'];
    if (files is! List || files.length != 1 || variables is! List || dependencies is! List) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidProposal);
    }
    final file = files.single;
    if (file is! AutomationJson ||
        file['content'] is! String ||
        file['path'] is! String ||
        variables.any((item) => item is! AutomationJson || item['name'] is! String || item['required'] is! bool) ||
        dependencies.any((item) => item is! AutomationJson)) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidProposal);
    }
    final draft = AutomationAiDraft._(
      file['content'] as String,
      [
        for (final item in variables.cast<AutomationJson>())
          if (item['required'] == true) item['name'] as String,
      ],
      [for (final item in dependencies.cast<AutomationJson>()) _dependency(item)],
    );
    draft.requirements(file['path'] as String);
    return draft;
  }

  AutomationAiDraft._(this.text, this.requiredVariables, this.dependencies);

  final String text;
  final List<String> requiredVariables;
  final List<AutomationJson> dependencies;
  bool get hasRequirements => requiredVariables.isNotEmpty || dependencies.isNotEmpty;

  void retainRequirements(AutomationAiDraft previous) {
    requiredVariables.addAll(previous.requiredVariables.where((name) => !requiredVariables.contains(name)));
    for (final old in previous.dependencies) {
      final matches = dependencies.where((item) => _dependencyKey(item) == _dependencyKey(old));
      if (matches.isEmpty) {
        dependencies.add(old);
      } else if (matches.single['version'] != old['version']) {
        throw const AutomationAiDraftException(AutomationAiDraftError.dependencyConflict);
      }
    }
    requirements('playbook.yml');
  }

  static AutomationJson _dependency(AutomationJson item) {
    final source = item['source'];
    if (source is! String || item['version'] is! String) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidProposal);
    }
    final parts = source.split(':');
    if (parts.length != 2) throw const AutomationAiDraftException(AutomationAiDraftError.invalidProposal);
    final result = <String, dynamic>{'kind': parts[0], 'name': parts[1], 'version': item['version']};
    _dependencyKey(result);
    return result;
  }

  /// Union existing constraints: regeneration cannot silently remove a manual
  /// variable/dependency requirement. Conflicting pins require a manual edit.
  String requirements(String path, {String? before}) {
    final document = before == null
        ? <String, dynamic>{'version': 1, 'playbooks': <String, dynamic>{}}
        : _strictJson(before);
    _validate(document);
    _path(path);
    final playbooks = document['playbooks'] as AutomationJson;
    final existing = playbooks[path] as AutomationJson?;
    final names = <String>{...?((existing?['required_extra_vars'] as List?)?.cast<String>()), ...requiredVariables};
    final pinned = <String, AutomationJson>{
      for (final item in (existing?['required_dependencies'] as List? ?? []).cast<AutomationJson>())
        _dependencyKey(item): item,
    };
    if (dependencies.map(_dependencyKey).toSet().length != dependencies.length) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
    }
    for (final dependency in dependencies) {
      final key = _dependencyKey(dependency);
      if (pinned[key] case final old?) {
        if (old['version'] != dependency['version']) {
          throw const AutomationAiDraftException(AutomationAiDraftError.dependencyConflict);
        }
      }
      pinned[key] = dependency;
    }
    final keys = pinned.keys.toList()..sort();
    playbooks[path] = {
      'required_extra_vars': names.toList()..sort(),
      'required_dependencies': [for (final key in keys) pinned[key]],
    };
    _validate(document);
    final result = '${const JsonEncoder.withIndent('  ').convert(document)}\n'; // l10n-ignore: serialized requirements JSON, not UI copy
    if (utf8.encode(result).length > 65536) {
      throw const AutomationAiDraftException(AutomationAiDraftError.limitExceeded);
    }
    return result;
  }
}

void _path(String path) {
  if (path.isEmpty ||
      path.length > 240 ||
      path.split('/').length > 16 ||
      path
          .split('/')
          .any((part) => part.isEmpty || part == '.' || part == '..' || !RegExp(r'^[A-Za-z0-9_.-]+$').hasMatch(part))) {
    throw const AutomationAiDraftException(AutomationAiDraftError.invalidPath);
  }
}

String _dependencyKey(AutomationJson item) {
  if (item.length != 3 || !item.keys.toSet().containsAll({'kind', 'name', 'version'})) {
    throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
  }
  final name = item['name'], version = item['version'];
  final identifier = RegExp(r'^[a-z][a-z0-9_]{0,63}$');
  if (name is! String ||
      version is! String ||
      !RegExp(r'^[A-Za-z0-9][A-Za-z0-9._+-]{0,79}$').hasMatch(version) ||
      version.toLowerCase() == 'latest' ||
      !(item['kind'] == 'role' && identifier.hasMatch(name) ||
          item['kind'] == 'collection' &&
              name != 'ansible.builtin' &&
              name.split('.').length == 2 &&
              name.split('.').every(identifier.hasMatch))) {
    throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
  }
  return '${item['kind']}:$name';
}

void _validate(AutomationJson document) {
  if (document.length != 2 ||
      document['version'] != 1 ||
      document['version'] is! int ||
      document['playbooks'] is! AutomationJson ||
      (document['playbooks'] as AutomationJson).length > 128) {
    throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
  }
  for (final entry in (document['playbooks'] as AutomationJson).entries) {
    _path(entry.key);
    final spec = entry.value;
    if (spec is! AutomationJson ||
        spec.length != 2 ||
        spec['required_extra_vars'] is! List ||
        spec['required_dependencies'] is! List) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
    }
    final names = spec['required_extra_vars'] as List, dependencies = spec['required_dependencies'] as List;
    if (names.length > 64 ||
        names.toSet().length != names.length ||
        names.any(
          (name) =>
              name is! String ||
              !RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,127}$').hasMatch(name) ||
              name.startsWith('ansible_') ||
              name.startsWith('consolecrypt_'),
        ) ||
        dependencies.length > 32) {
      throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
    }
    final keys = <String>{};
    for (final item in dependencies) {
      if (item is! AutomationJson || !keys.add(_dependencyKey(item))) {
        throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
      }
    }
  }
}

/// jsonDecode validates grammar; this token walk additionally rejects duplicate
/// keys (including escaped spellings) before merging a manually edited file.
AutomationJson _strictJson(String raw) {
  if (utf8.encode(raw).length > 65536) throw const AutomationAiDraftException(AutomationAiDraftError.limitExceeded);
  final Object? value;
  try {
    value = jsonDecode(raw);
  } on FormatException {
    throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
  }
  if (value is! AutomationJson) throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
  final keys = <Set<String>?>[], expectingKey = <bool>[];
  final tokens = RegExp(r'"(?:[^"\\]|\\.)*"|[{}\[\],:]');
  for (final match in tokens.allMatches(raw)) {
    final token = match[0]!;
    if (token == '{' || token == '[') {
      keys.add(token == '{' ? <String>{} : null);
      expectingKey.add(token == '{');
    } else if (token == '}' || token == ']') {
      keys.removeLast();
      expectingKey.removeLast();
    } else if (keys.isNotEmpty && keys.last != null) {
      if (token == ',') expectingKey[expectingKey.length - 1] = true;
      if (token == ':') expectingKey[expectingKey.length - 1] = false;
      if (token.startsWith('"') && expectingKey.last && !keys.last!.add(jsonDecode(token) as String)) {
        throw const AutomationAiDraftException(AutomationAiDraftError.invalidRequirements);
      }
    }
  }
  return value;
}
