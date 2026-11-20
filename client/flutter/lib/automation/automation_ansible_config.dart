import 'dart:convert';

import 'package:consolecrypt/core/l10n/l10n.dart';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Intentionally small: only these three [defaults] settings have form controls.
/// Native/Runner override inventory, forks and SSH trust. Other existing INI
/// settings retain their Ansible semantics; there is no general CFG allowlist.
/// See ansible-core 2.20's reference_appendices/config.html.
enum AutomationAnsibleSetting {
  gathering('gathering'),
  anyErrorsFatal('any_errors_fatal'),
  taskTimeout('task_timeout'),
  inventory('inventory'),
  rolesPath('roles_path'),
  vaultPasswordFile('vault_password_file');

  const AutomationAnsibleSetting(this.iniKey);
  final String iniKey;

  /// Canonical UI value, or null for a value requiring an explicit replacement.
  String? canonical(String? value) {
    if (value == null) return null;
    return switch (this) {
      gathering => const ['implicit', 'explicit', 'smart'].contains(value) ? value : null,
      anyErrorsFatal => switch (value.toLowerCase()) {
        'true' || 'yes' || 'on' || '1' => 'True',
        'false' || 'no' || 'off' || '0' => 'False',
        _ => null,
      },
      taskTimeout =>
        RegExp(r'^\d+$').hasMatch(value) && int.tryParse(value) != null ? int.parse(value).toString() : null,
      inventory || rolesPath || vaultPasswordFile => null,
    };
  }
}

class _ConfigLine {
  const _ConfigLine(this.start, this.content, this.ending);
  final int start;
  final String content;
  final String ending;
  int get end => start + content.length + ending.length;
}

class _ConfigValue {
  const _ConfigValue(this.start, this.end, this.value, this.needsCommentSpace);
  final int start;
  final int end;
  final String value;
  final bool needsCommentSpace;
}

/// A conservative, lossless patch model, not an alternative Ansible parser.
/// Never writes on parse. Unknown lines, comments, BOM, mixed line endings and
/// absent final newlines remain exact. Ambiguous INI requires the advanced editor.
class AutomationAnsibleConfigDocument {
  AutomationAnsibleConfigDocument._(
    this.text,
    this.canEdit,
    this._values,
    this._insertAt,
    this._hasDefaults,
    this._newline,
  );

  factory AutomationAnsibleConfigDocument.parse(String text) {
    final values = <AutomationAnsibleSetting, _ConfigValue>{};
    if (text.length > 1024 * 1024 || utf8.encode(text).length > 1024 * 1024 || text.contains('\u0000')) {
      return AutomationAnsibleConfigDocument._(text, false, values, 0, false, '\n');
    }
    final lines = <_ConfigLine>[];
    var start = 0;
    for (final match in RegExp(r'\r\n|\n|\r').allMatches(text)) {
      lines.add(_ConfigLine(start, text.substring(start, match.start), match.group(0)!));
      start = match.end;
    }
    if (start < text.length) lines.add(_ConfigLine(start, text.substring(start), ''));
    var safe = true;
    var section = '';
    var insertAt = text.length;
    var hasDefaults = false;
    final sections = <String>{};
    final keys = <String>{};
    int? previousIndent;
    AutomationAnsibleSetting? previousSetting;
    for (final line in lines) {
      final bom = line.start == 0 && line.content.startsWith('\ufeff') ? 1 : 0;
      final content = line.content.substring(bom);
      final trimmed = content.trimLeft();
      if (trimmed.isEmpty || trimmed.startsWith('#') || trimmed.startsWith(';')) continue;
      final indent = content.length - trimmed.length;
      if (previousIndent != null && indent > previousIndent) {
        if (previousSetting != null) safe = false;
        continue;
      }
      final header = RegExp(r'^\s*\[([^\]]+)\]\s*(?:[;#].*)?$').firstMatch(content);
      if (header != null) {
        section = header.group(1)!;
        if (!sections.add(section) || section == 'DEFAULT') safe = false;
        keys.clear();
        previousIndent = null;
        previousSetting = null;
        if (section == 'defaults') {
          hasDefaults = true;
          insertAt = line.end;
        }
        continue;
      }
      final assignment = RegExp(r'^([ \t]*([^:=]+?)[ \t]*[=:][ \t]*)(.*)$').firstMatch(content);
      if (section.isEmpty || assignment == null || trimmed.startsWith('[')) {
        safe = false;
        continue;
      }
      final key = assignment.group(2)!.trim().toLowerCase();
      if (key.isEmpty || !keys.add(key)) safe = false;
      previousIndent = indent;
      previousSetting = section == 'defaults'
          ? AutomationAnsibleSetting.values.where((setting) => setting.iniKey == key).firstOrNull
          : null;
      if (previousSetting == null) continue;
      final raw = assignment.group(3)!;
      // Ansible's ConfigParser allows inline ';' only after whitespace. '#'
      // remains part of the value. Do not silently convert it to a comment.
      final comment = RegExp(r'(^|[ \t]);').firstMatch(raw);
      final valuePart = raw.substring(0, comment?.start ?? raw.length);
      final value = valuePart.trimRight();
      final offset = line.start + bom + assignment.group(1)!.length;
      values[previousSetting] = _ConfigValue(
        offset,
        offset + value.length,
        value,
        value.isEmpty && raw.startsWith(';'),
      );
    }
    final newline = lines.where((line) => line.ending.isNotEmpty).firstOrNull?.ending ?? '\n';
    return AutomationAnsibleConfigDocument._(text, safe, Map.unmodifiable(values), insertAt, hasDefaults, newline);
  }

  final String text;
  final bool canEdit;
  final Map<AutomationAnsibleSetting, _ConfigValue> _values;
  final int _insertAt;
  final bool _hasDefaults;
  final String _newline;

  String? value(AutomationAnsibleSetting setting) => _values[setting]?.value;

  /// Only the explicitly supplied values change. Empty changes always round trip,
  /// even for a document the form cannot safely edit. No backend save is performed.
  String propose(Map<AutomationAnsibleSetting, String> changes) {
    if (changes.isEmpty) return text;
    if (!canEdit) throw StateError('ambiguous_ini');
    final replacements = <({int start, int end, String text})>[];
    final additions = <String>[];
    for (final setting in AutomationAnsibleSetting.values) {
      final requested = changes[setting];
      if (requested == null) continue;
      final canonical = setting.canonical(requested);
      if (canonical == null) throw ArgumentError.value(requested, setting.iniKey);
      final current = _values[setting];
      // Selecting the same semantic value does not canonicalize existing text.
      if (canonical == setting.canonical(current?.value)) continue;
      if (current == null) {
        additions.add('${setting.iniKey} = $canonical');
      } else {
        replacements.add((
          start: current.start,
          end: current.end,
          text: '$canonical${current.needsCommentSpace ? ' ' : ''}',
        ));
      }
    }
    if (additions.isNotEmpty) {
      final at = _hasDefaults ? _insertAt : text.length;
      final needsNewline = at > 0 && text[at - 1] != '\n' && text[at - 1] != '\r' && text[at - 1] != '\ufeff';
      final prefix = '${needsNewline ? _newline : ''}${_hasDefaults ? '' : '[defaults]$_newline'}';
      replacements.add((start: at, end: at, text: '$prefix${additions.join(_newline)}$_newline'));
    }
    replacements.sort((a, b) => b.start.compareTo(a.start));
    var result = text;
    for (final replacement in replacements) {
      result = result.replaceRange(replacement.start, replacement.end, replacement.text);
    }
    return result;
  }
}

enum AutomationAnsibleConfigAction { apply, advanced }

/// Caller retains the original file SHA and uses its existing CAS save API.
/// An advanced result contains no proposal: open the original text editor.
class AutomationAnsibleConfigResult {
  const AutomationAnsibleConfigResult.apply(String text)
    : action = AutomationAnsibleConfigAction.apply,
      proposedText = text;
  const AutomationAnsibleConfigResult.advanced() : action = AutomationAnsibleConfigAction.advanced, proposedText = null;
  final AutomationAnsibleConfigAction action;
  final String? proposedText;
}

/// Present inside the caller's existing AutomationScopeGuard; this widget never
/// reads files, saves configuration, starts a job or holds credentials.
class AutomationAnsibleConfigDialog extends StatefulWidget {
  const AutomationAnsibleConfigDialog({super.key, required this.text, this.hasFile = true});
  final String text;
  final bool hasFile;

  @override
  State<AutomationAnsibleConfigDialog> createState() => _AutomationAnsibleConfigDialogState();
}

class _AutomationAnsibleConfigDialogState extends State<AutomationAnsibleConfigDialog> {
  late final AutomationAnsibleConfigDocument document = AutomationAnsibleConfigDocument.parse(widget.text);
  final Map<AutomationAnsibleSetting, String> changes = {};
  int resetEpoch = 0;
  late final TextEditingController timeout = TextEditingController(
    text: AutomationAnsibleSetting.taskTimeout.canonical(document.value(AutomationAnsibleSetting.taskTimeout)) ?? '',
  );

  @override
  void dispose() {
    timeout.dispose();
    super.dispose();
  }

  String? current(AutomationAnsibleSetting setting) => changes[setting] ?? setting.canonical(document.value(setting));

  void change(AutomationAnsibleSetting setting, String value) {
    setState(() {
      if (setting.canonical(value) == setting.canonical(document.value(setting)) && setting.canonical(value) != null) {
        changes.remove(setting);
      } else {
        changes[setting] = value;
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final words = _ConfigWords(context.l10n);
    final timeoutChange = changes[AutomationAnsibleSetting.taskTimeout];
    final valid = timeoutChange == null || AutomationAnsibleSetting.taskTimeout.canonical(timeoutChange) != null;
    String status(AutomationAnsibleSetting setting) => document.value(setting) == null ? words.inherited : words.custom;
    Widget select(AutomationAnsibleSetting setting, String label, String help, List<DropdownMenuItem<String>> items) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 20),
        child: DropdownButtonFormField<String>(
          key: ValueKey((setting.iniKey, resetEpoch)),
          initialValue: current(setting),
          isExpanded: true,
          decoration: InputDecoration(labelText: label, helperText: help, helperMaxLines: 4),
          hint: Text(status(setting)),
          items: items,
          onChanged: document.canEdit
              ? (value) {
                  if (value != null) change(setting, value);
                }
              : null,
        ),
      );
    }

    return AlertDialog(
      title: Text(words.title),
      content: SizedBox(
        width: 580,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(widget.hasFile ? context.l10n.automationConfigFromFile : context.l10n.automationConfigNewFile),
              const SizedBox(height: 8),
              Text(words.policy),
              const SizedBox(height: 16),
              Text(words.precedence),
              const SizedBox(height: 20),
              if (!document.canEdit) ...[Text(words.complex), const SizedBox(height: 20)],
              select(AutomationAnsibleSetting.gathering, words.gathering, words.gatherHelp, [
                DropdownMenuItem(value: 'implicit', child: Text(words.implicit)),
                DropdownMenuItem(value: 'explicit', child: Text(words.explicit)),
                DropdownMenuItem(value: 'smart', child: Text(words.smart)),
              ]),
              select(AutomationAnsibleSetting.anyErrorsFatal, words.fatal, words.fatalHelp, [
                DropdownMenuItem(value: 'False', child: Text(words.no)),
                DropdownMenuItem(value: 'True', child: Text(words.yes)),
              ]),
              TextField(
                key: const ValueKey('task_timeout'),
                controller: timeout,
                enabled: document.canEdit,
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(10)],
                decoration: InputDecoration(
                  labelText: words.timeout,
                  hintText: status(AutomationAnsibleSetting.taskTimeout),
                  helperText: words.timeoutHelp,
                  helperMaxLines: 4,
                  errorText: valid ? null : words.invalidTimeout,
                ),
                onChanged: (value) => change(AutomationAnsibleSetting.taskTimeout, value),
              ),
              const SizedBox(height: 20),
              for (final setting in [
                AutomationAnsibleSetting.inventory,
                AutomationAnsibleSetting.rolesPath,
                AutomationAnsibleSetting.vaultPasswordFile,
              ])
                if (document.value(setting) case final value?)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: SelectableText('${setting.iniKey} = $value'),
                  ),
              Text(words.preserve),
              if (changes.isNotEmpty)
                TextButton(
                  onPressed: () => setState(() {
                    changes.clear();
                    resetEpoch++;
                    timeout.text =
                        AutomationAnsibleSetting.taskTimeout.canonical(
                          document.value(AutomationAnsibleSetting.taskTimeout),
                        ) ??
                        '';
                  }),
                  child: Text(words.reset),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(const AutomationAnsibleConfigResult.advanced()),
          child: Text(words.advanced),
        ),
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(words.cancel)),
        FilledButton(
          onPressed: document.canEdit && changes.isNotEmpty && valid
              ? () => Navigator.of(context).pop(AutomationAnsibleConfigResult.apply(document.propose(changes)))
              : null,
          child: Text(words.apply),
        ),
      ],
    );
  }
}

class _ConfigWords {
  const _ConfigWords(this.l);
  final AppLocalizations l;
  String get title => l.automationConfigTitle;
  String get policy => l.automationConfigPolicy;
  String get precedence => l.automationConfigPrecedence;
  String get complex => l.automationConfigComplex;
  String get gathering => l.automationConfigGathering;
  String get gatherHelp => l.automationConfigGatherHelp;
  String get implicit => l.automationConfigImplicit;
  String get explicit => l.automationConfigExplicit;
  String get smart => l.automationConfigSmart;
  String get fatal => l.automationConfigFatal;
  String get fatalHelp => l.automationConfigFatalHelp;
  String get timeout => l.automationConfigTimeout;
  String get timeoutHelp => l.automationConfigTimeoutHelp;
  String get invalidTimeout => l.automationConfigInvalidTimeout;
  String get inherited => l.automationConfigInherited;
  String get custom => l.automationConfigCustom;
  String get preserve => l.automationConfigPreserve;
  String get yes => l.automationConfigYes;
  String get no => l.automationConfigNo;
  String get reset => l.automationConfigReset;
  String get advanced => l.automationConfigAdvanced;
  String get cancel => l.automationConfigCancel;
  String get apply => l.automationConfigApply;
}
