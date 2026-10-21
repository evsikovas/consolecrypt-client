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

  /// One row: run (with its modes), cancel, then editing and plan tools.
  Widget _toolbar(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    Widget tool(String key, IconData icon, String tooltip, VoidCallback? onPressed) => GlassIconButton(
      key: ValueKey(key),
      icon: icon,
      tooltip: tooltip,
      size: 30,
      iconSize: 17,
      style: GlassIconButtonStyle.plain,
      onPressed: onPressed,
    );
    Widget divider() => Container(
      width: 1,
      height: 18,
      margin: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
      color: tokens.surfaces.separator,
    );
    final sql = widget.session.engine != DbEngine.redis;
    return Padding(
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s6, GlassSpacing.s6, GlassSpacing.s6, GlassSpacing.s4),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            GlassButton(
              key: const ValueKey('db-run'),
              label: l.dbRun,
              icon: Icons.play_arrow_rounded,
              style: GlassButtonStyle.prominent,
              size: GlassControlSize.sm,
              tooltip: '${l.dbRunCurrent} (⌘↵)', // l10n-ignore: shortcut glyphs
              onPressed: widget.running ? null : () => widget.onRun(DbRunMode.current),
            ),
            GlassMenuButton<DbRunMode>(
              key: const ValueKey('db-run-modes'),
              entries: [
                GlassMenuItem(value: DbRunMode.current, label: l.dbRunCurrent, icon: Icons.play_arrow_rounded),
                GlassMenuItem(value: DbRunMode.selection, label: l.dbRunSelection, icon: Icons.highlight_alt_rounded),
                GlassMenuItem(value: DbRunMode.script, label: l.dbRunScript, icon: Icons.playlist_play_rounded),
              ],
              onSelected: (mode) {
                if (!widget.running) widget.onRun(mode);
              },
              builder: (context, open) => tool('db-run-menu', Icons.expand_more_rounded, l.dbRunScript, open),
            ),
            tool('db-cancel', Icons.stop_rounded, l.dbCancelQuery, widget.running ? widget.onCancel : null),
            divider(),
            if (widget.onExplain != null && sql)
              GlassMenuButton<bool>(
                key: const ValueKey('db-explain-menu'),
                entries: [
                  GlassMenuItem(value: false, label: l.dbExplain, icon: Icons.account_tree_outlined),
                  if (!widget.session.production)
                    GlassMenuItem(value: true, label: l.dbExplainAnalyze, icon: Icons.speed_rounded),
                ],
                onSelected: widget.onExplain!,
                builder: (context, open) => tool('db-explain', Icons.account_tree_outlined, l.dbExplain, open),
              ),
            tool('db-format', Icons.format_align_left_rounded, l.dbFormat, _busy ? null : _format),
            tool('db-complete', Icons.auto_fix_high_rounded, l.dbComplete, _busy ? null : _complete),
            if (sql) tool('db-comment', Icons.code_rounded, l.dbComment, _comments),
            tool('db-find', Icons.find_replace_rounded, l.dbFindReplace, () => setState(() => _findOpen = !_findOpen)),
            if (widget.onAi != null) ...[divider(), tool('db-ai', Icons.auto_awesome_rounded, l.dbAi, widget.onAi)],
          ],
        ),
      ),
    );
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
          _toolbar(context),
          Container(height: 1, color: GlassTokens.of(context).surfaces.separator),
          if (_busy) const LinearProgressIndicator(),
          if (_findOpen)
            Padding(
              padding: const EdgeInsets.all(4),
              child: Wrap(
                spacing: 6,
                children: [
                  SizedBox(
                    width: 180,
                    child: GlassField(controller: _find, search: true, placeholder: l.commonSearch),
                  ),
                  SizedBox(
                    width: 180,
                    child: GlassField(controller: _replace, placeholder: l.dbReplace),
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
            child: Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s12, GlassSpacing.s8),
              child: TextField(
                key: const ValueKey('db-query-editor'),
                controller: widget.controller,
                focusNode: _focus,
                expands: true,
                maxLines: null,
                minLines: null,
                decoration: InputDecoration.collapsed(hintText: l.dbEditorHint),
                style: GlassTokens.of(context).typography.mono.copyWith(fontSize: 13.5, height: 1.45),
                textAlignVertical: TextAlignVertical.top,
                autocorrect: false,
                enableSuggestions: false,
                smartDashesType: SmartDashesType.disabled,
                smartQuotesType: SmartQuotesType.disabled,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
