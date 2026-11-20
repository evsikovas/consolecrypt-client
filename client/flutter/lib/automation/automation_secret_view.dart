import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_syntax.dart';
import 'package:material_ui/material_ui.dart';

/// Display redaction only. The saved document is always the original controller.
/// This deliberately does not claim to find arbitrary secrets in free text.
String maskAutomationPasswords(String source) {
  final units = source.codeUnits.toList();
  final keys = RegExp(
    r'''\b(?:ansible_(?:password|ssh_pass(?:word)?|become_pass(?:word)?|sudo_pass|su_pass|vault_password)|password|passwd|api_key|access_token|secret_key)\b["']?[ \t]*([=:])[ \t]*''',
    caseSensitive: false,
  );
  var covered = 0;
  void mask(int start, int end) {
    for (var i = start; i < end; i++) {
      // Preserve layout and UTF-16 offsets, including surrogate pairs.
      if (units[i] != 10 && units[i] != 13) units[i] = 0x2022;
    }
    covered = end;
  }

  void continuation(int keyStart, int afterValue) {
    final first = source.indexOf('\n', afterValue);
    if (first < 0) return;
    final lineStart = source.lastIndexOf('\n', keyStart) + 1;
    final prefix = source.substring(lineStart, keyStart);
    final indent = prefix.length - prefix.trimLeft().length;
    var cursor = first + 1;
    while (cursor < source.length) {
      final next = source.indexOf('\n', cursor);
      final stop = next < 0 ? source.length : next + 1;
      final line = source.substring(cursor, stop);
      final leading = line.length - line.trimLeft().length;
      if (line.trim().isNotEmpty && leading <= indent) break;
      if (line.trim().isNotEmpty) mask(cursor + leading, stop);
      cursor = stop;
    }
    covered = cursor;
  }

  for (final match in keys.allMatches(source)) {
    if (match.start < covered) continue;
    final lineStart = source.lastIndexOf('\n', match.start) + 1;
    final prefix = source.substring(lineStart, match.start).trimLeft();
    if (prefix.startsWith('#') || prefix.startsWith(';')) continue;
    final start = match.end;
    if (start == source.length) continue;
    if (source[start] == '\n' || source[start] == '\r') {
      if (match.group(1) == ':') continuation(match.start, start);
      continue;
    }
    final first = source[start];
    if (first == '"' || first == "'") {
      var end = start + 1;
      while (end < source.length) {
        if (first == '"' && source[end] == '\\') {
          end = (end + 2).clamp(0, source.length);
        } else if (source[end] == first) {
          if (first == "'" && end + 1 < source.length && source[end + 1] == "'") {
            end += 2;
          } else {
            break;
          }
        } else {
          end++;
        }
      }
      mask(start + 1, end);
    } else if (match.group(1) == ':' && (first == '|' || first == '>')) {
      final lineEnd = source.indexOf('\n', start);
      if (lineEnd < 0) continue;
      final leading = source.substring(lineStart, match.start);
      final indentation = leading.length - leading.trimLeft().length;
      var end = lineEnd + 1;
      while (end < source.length) {
        final next = source.indexOf('\n', end);
        final stop = next < 0 ? source.length : next + 1;
        final line = source.substring(end, stop);
        if (line.trim().isNotEmpty && line.length - line.trimLeft().length <= indentation) break;
        end = stop;
      }
      mask(lineEnd + 1, end);
    } else {
      var end = start;
      while (end < source.length && source[end] != '\n' && source[end] != '\r') {
        if (match.group(1) == '=' && source[end] == '\\' && end + 1 < source.length && source[end + 1] != '\n') {
          end += 2;
          continue;
        }
        if (match.group(1) == '=' && const [9, 11, 12, 32].contains(source.codeUnitAt(end))) break;
        if (source[end] == '#' && (end == start || source[end - 1] == ' ')) break;
        if (match.group(1) == ':' && (source[end] == ',' || source[end] == '}')) break;
        end++;
      }
      while (end > start && (source[end - 1] == ' ' || source[end - 1] == '\t')) {
        end--;
      }
      mask(start, end);
      if (match.group(1) == ':') continuation(match.start, end);
    }
  }
  return String.fromCharCodes(units);
}

/// Hidden mode uses a different, masked controller, including accessibility and
/// clipboard selection. Editing requires revealing: bullets never replace data.
class AutomationSecretView extends StatefulWidget {
  const AutomationSecretView({super.key, required this.controller, required this.builder});
  final TextEditingController controller;
  final Widget Function(TextEditingController display, bool masked) builder;
  @override
  State<AutomationSecretView> createState() => _AutomationSecretViewState();
}

class _AutomationSecretViewState extends State<AutomationSecretView> {
  final masked = KubeYamlController();
  bool revealed = false;
  Timer? revealTimer;
  String lastText = '';
  bool get hasSecrets => masked.text != widget.controller.text;
  @override
  void initState() {
    super.initState();
    lastText = widget.controller.text;
    masked.text = maskAutomationPasswords(lastText);
    widget.controller.addListener(_changed);
  }

  void _changed() {
    if (lastText == widget.controller.text) return;
    lastText = widget.controller.text;
    final next = maskAutomationPasswords(lastText);
    masked.text = next;
    setState(() {});
  }

  @override
  void didUpdateWidget(AutomationSecretView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_changed);
      widget.controller.addListener(_changed);
      revealed = false;
      revealTimer?.cancel();
      lastText = widget.controller.text;
      masked.text = maskAutomationPasswords(lastText);
    }
  }

  @override
  void dispose() {
    revealTimer?.cancel();
    widget.controller.removeListener(_changed);
    masked.clear();
    masked.dispose();
    lastText = '';
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final hidden = hasSecrets && !revealed;
    return RevealedSecret(
      visible: hasSecrets && revealed,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (hasSecrets) ...[
            Row(
              children: [
                const Icon(Icons.lock_outline, size: 16),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    hidden ? context.l10n.automationPasswordsHidden : context.l10n.automationPasswordsVisible,
                  ),
                ),
                TextButton.icon(
                  key: const ValueKey('automation-toggle-secrets'),
                  onPressed: () {
                    FocusManager.instance.primaryFocus?.unfocus();
                    setState(() => revealed = !revealed);
                    revealTimer?.cancel();
                    if (revealed) {
                      revealTimer = Timer(const Duration(seconds: 20), () {
                        if (mounted) setState(() => revealed = false);
                      });
                    }
                  },
                  icon: Icon(hidden ? Icons.visibility_outlined : Icons.visibility_off_outlined, size: 16),
                  label: Text(hidden ? context.l10n.commonShow : context.l10n.commonHide),
                ),
              ],
            ),
            const SizedBox(height: 6),
          ],
          Expanded(child: widget.builder(hidden ? masked : widget.controller, hidden)),
        ],
      ),
    );
  }
}
