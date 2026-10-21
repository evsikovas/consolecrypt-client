import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Highlighting is presentation only; native dialect parsing defines statement
/// boundaries, parameter positions and the security classification.
class DbEditorController extends TextEditingController {
  DbEditorController({super.text});
  static final _tokens = RegExp(
    r'''(--[^\n]*|//[^\n]*|/\*[\s\S]*?\*/|'(?:''|[^'])*'|\"(?:\\.|[^\"])*\"|\b(?:SELECT|FROM|WHERE|JOIN|LEFT|RIGHT|GROUP|BY|ORDER|LIMIT|OFFSET|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP|WITH|AS|AND|OR|NOT|NULL|TRUE|FALSE|SET|GET|SCAN|HGETALL|ZRANGE|FIND|AGGREGATE|MATCH|PROJECT|SORT)\b|\{\{[A-Za-z_][A-Za-z0-9_]*\}\}|\b\d+(?:\.\d+)?\b)''',
    caseSensitive: false,
  );
  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    // Keep very large scripts editable without a regexp pass on every frame.
    if (text.length > 131072 || (withComposing && value.composing.isValid && !value.composing.isCollapsed)) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    final children = <InlineSpan>[];
    var end = 0;
    for (final match in _tokens.allMatches(text)) {
      if (match.start > end) children.add(TextSpan(text: text.substring(end, match.start)));
      final token = match[0]!;
      final color = token.startsWith('--') || token.startsWith('//') || token.startsWith('/*')
          ? Theme.of(context).hintColor
          : token.startsWith("'") || token.startsWith('"')
          ? Theme.of(context).colorScheme.tertiary
          : Theme.of(context).colorScheme.primary;
      children.add(
        TextSpan(
          text: token,
          style: TextStyle(color: color),
        ),
      );
      end = match.end;
    }
    children.add(TextSpan(text: text.substring(end)));
    return TextSpan(style: style, children: children);
  }

  void insert(String value) {
    final s = selection.isValid ? selection : TextSelection.collapsed(offset: text.length);
    this.value = TextEditingValue(
      text: text.replaceRange(s.start, s.end, value),
      selection: TextSelection.collapsed(offset: s.start + value.length),
    );
  }
}

class DbQueryEditor extends StatefulWidget {
  const DbQueryEditor({
    required this.controller,
    required this.session,
    required this.service,
    required this.onRun,
    required this.isCurrent,
    this.running = false,
    this.onCancel,
    this.onAi,
    this.onExplain,
    this.onHistory,
    super.key,
  });
  final DbEditorController controller;
  final DbSessionInfo session;
  final DbQueryService service;
  final ValueChanged<DbRunMode> onRun;
  final bool Function() isCurrent;
  final bool running;
  final VoidCallback? onCancel, onAi, onHistory;
  final ValueChanged<bool>? onExplain;
  @override
  State<DbQueryEditor> createState() => _DbQueryEditorState();
}

class _DbQueryEditorState extends State<DbQueryEditor> {
  final _focus = FocusNode();
  final _find = TextEditingController(), _replace = TextEditingController();
  bool _findOpen = false, _busy = false;
  int _generation = 0;
  bool get _current => mounted && widget.isCurrent();
  @override
  void dispose() {
    _generation++;
    _focus.dispose();
    _find.clear();
    _find.dispose();
    _replace.clear();
    _replace.dispose();
    super.dispose();
  }

  Future<void> _format() async {
    if (_busy || !_current) return;
    final original = widget.controller.text, generation = ++_generation;
    setState(() => _busy = true);
    try {
      final formatted = await widget.service.formatQuery(widget.session.engine, original);
      if (_current && generation == _generation && widget.controller.text == original) {
        widget.controller.text = formatted;
      }
    } catch (_) {
      if (_current) _error();
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  void _error() => ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(context.l10n.dbOperationFailed)));
  void _comments() {
    final controller = widget.controller, s = widget.controller.selection;
    if (!s.isValid) return;
    final start = s.start == 0 ? 0 : controller.text.lastIndexOf('\n', s.start - 1) + 1;
    final found = controller.text.indexOf('\n', s.end), end = found < 0 ? controller.text.length : found;
    final prefix = widget.session.engine == DbEngine.mongodb ? '// ' : '-- ';
    final lines = controller.text.substring(start, end).split('\n');
    final remove = lines.every((line) => line.trimLeft().startsWith(prefix.trim()));
    final replacement = lines
        .map(
          (line) => remove
              ? line.replaceFirstMapped(RegExp('^([ \\t]*)${RegExp.escape(prefix.trim())} ?'), (m) => m[1]!)
              : '$prefix$line',
        )
        .join('\n');
    controller.value = TextEditingValue(
      text: controller.text.replaceRange(start, end, replacement),
      selection: TextSelection(baseOffset: start, extentOffset: start + replacement.length),
    );
  }

  void _replaceText(bool all) {
    if (_find.text.isEmpty) return;
    final controller = widget.controller;
    if (all) {
      controller.text = controller.text.replaceAll(_find.text, _replace.text);
      return;
    }
    final start = controller.selection.isValid ? controller.selection.start : 0;
    var index = controller.text.indexOf(_find.text, start);
    if (index < 0) index = controller.text.indexOf(_find.text);
    if (index >= 0) {
      controller.selection = TextSelection(baseOffset: index, extentOffset: index + _find.text.length);
      controller.insert(_replace.text);
    }
  }

  Future<void> _complete() async {
    if (!_current || _busy) return;
    final text = widget.controller.text, selection = widget.controller.selection;
    final generation = ++_generation;
    setState(() => _busy = true);
    try {
      final completions = await widget.service.completeQuery(
        widget.session.id,
        text,
        selection.isValid ? selection.extentOffset : text.length,
      );
      if (!mounted || !_current || generation != _generation || widget.controller.text != text) return;
      final completion = await showAppDialog<DbCompletion>(
        context,
        secure: true,
        builder: (context) => GlassDialog(
          title: context.l10n.dbComplete,
          content: SizedBox(
            width: 520,
            height: 360,
            child: ListView.builder(
              itemCount: completions.length.clamp(0, 100),
              itemBuilder: (_, i) => ListTile(
                title: Text(completions[i].label),
                subtitle: Text(completions[i].detail),
                onTap: () => closeDialog(context, completions[i]),
              ),
            ),
          ),
          secondaryActions: [GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context))],
        ),
      );
      if (_current &&
          generation == _generation &&
          widget.controller.text == text &&
          widget.controller.selection == selection &&
          completion != null) {
        widget.controller.insert(completion.insertText);
      }
    } catch (_) {
      if (_current) _error();
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return CallbackShortcuts(
      bindings: {
        for (final meta in [false, true]) ...{
          SingleActivator(LogicalKeyboardKey.enter, control: !meta, meta: meta): () => widget.onRun(
            widget.controller.selection.isValid && !widget.controller.selection.isCollapsed
                ? DbRunMode.selection
                : DbRunMode.current,
          ),
          SingleActivator(LogicalKeyboardKey.space, control: !meta, meta: meta): () => unawaited(_complete()),
          SingleActivator(LogicalKeyboardKey.keyF, control: !meta, meta: meta): () =>
              setState(() => _findOpen = !_findOpen),
          SingleActivator(LogicalKeyboardKey.slash, control: !meta, meta: meta): _comments,
          if (widget.onAi != null) SingleActivator(LogicalKeyboardKey.keyI, control: !meta, meta: meta): widget.onAi!,
          SingleActivator(LogicalKeyboardKey.keyF, control: !meta, meta: meta, shift: true): () {
            if (!_busy) unawaited(_format());
          },
          if (widget.onCancel != null)
            SingleActivator(LogicalKeyboardKey.period, control: !meta, meta: meta): widget.onCancel!,
          if (widget.onExplain != null)
            SingleActivator(LogicalKeyboardKey.keyE, control: !meta, meta: meta): () => widget.onExplain!(false),
          if (widget.onHistory != null)
            SingleActivator(LogicalKeyboardKey.keyH, control: !meta, meta: meta): widget.onHistory!,
        },
      },
      child: Column(
        children: [
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                TextButton.icon(
                  key: const ValueKey('db-run'),
                  onPressed: widget.running ? null : () => widget.onRun(DbRunMode.current),
                  icon: const Icon(Icons.play_arrow),
                  label: Text(l.dbRun),
                ),
                PopupMenuButton<DbRunMode>(
                  onSelected: widget.running ? null : widget.onRun,
                  itemBuilder: (_) => [
                    PopupMenuItem(value: DbRunMode.current, child: Text(l.dbRunCurrent)),
                    PopupMenuItem(value: DbRunMode.selection, child: Text(l.dbRunSelection)),
                    PopupMenuItem(value: DbRunMode.script, child: Text(l.dbRunScript)),
                  ],
                ),
                if (widget.running)
                  IconButton(tooltip: l.dbCancelQuery, onPressed: widget.onCancel, icon: const Icon(Icons.stop)),
                IconButton(
                  tooltip: l.dbFormat,
                  onPressed: _busy ? null : _format,
                  icon: const Icon(Icons.format_align_left),
                ),
                IconButton(
                  tooltip: l.dbComplete,
                  onPressed: _busy ? null : _complete,
                  icon: const Icon(Icons.auto_fix_high),
                ),
                if (widget.session.engine != DbEngine.redis)
                  IconButton(tooltip: l.dbComment, onPressed: _comments, icon: const Icon(Icons.code)),
                IconButton(
                  tooltip: l.dbFindReplace,
                  onPressed: () => setState(() => _findOpen = !_findOpen),
                  icon: const Icon(Icons.find_replace),
                ),
                if (widget.onExplain != null && widget.session.engine != DbEngine.redis)
                  PopupMenuButton<bool>(
                    tooltip: l.dbExplain,
                    icon: const Icon(Icons.account_tree),
                    onSelected: widget.onExplain,
                    itemBuilder: (_) => [
                      PopupMenuItem(value: false, child: Text(l.dbExplain)),
                      if (!widget.session.production) PopupMenuItem(value: true, child: Text(l.dbExplainAnalyze)),
                    ],
                  ),
                if (widget.onAi != null)
                  IconButton(tooltip: l.dbAi, onPressed: widget.onAi, icon: const Icon(Icons.auto_awesome)),
              ],
            ),
          ),
          if (_busy) const LinearProgressIndicator(),
          if (_findOpen)
            Padding(
              padding: const EdgeInsets.all(4),
              child: Wrap(
                spacing: 6,
                children: [
                  SizedBox(
                    width: 150,
                    child: TextField(
                      controller: _find,
                      decoration: InputDecoration(labelText: l.commonSearch),
                    ),
                  ),
                  SizedBox(
                    width: 150,
                    child: TextField(
                      controller: _replace,
                      decoration: InputDecoration(labelText: l.dbReplace),
                    ),
                  ),
                  IconButton(
                    tooltip: l.dbReplace,
                    onPressed: () => _replaceText(false),
                    icon: const Icon(Icons.find_replace),
                  ),
                  TextButton(onPressed: () => _replaceText(true), child: Text(l.dbReplaceAll)),
                ],
              ),
            ),
          Expanded(
            child: TextField(
              key: const ValueKey('db-query-editor'),
              controller: widget.controller,
              focusNode: _focus,
              expands: true,
              maxLines: null,
              minLines: null,
              decoration: InputDecoration(labelText: l.dbEditor, alignLabelWithHint: true),
              style: const TextStyle(fontFamily: 'monospace', fontSize: 14),
              textAlignVertical: TextAlignVertical.top,
              autocorrect: false,
              enableSuggestions: false,
              smartDashesType: SmartDashesType.disabled,
              smartQuotesType: SmartQuotesType.disabled,
            ),
          ),
        ],
      ),
    );
  }
}
