import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

final _highlightTokens = RegExp(
  r'''(--[^\n]*|//[^\n]*|/\*[\s\S]*?\*/|'(?:''|[^'])*'|\"(?:\\.|[^\"])*\"|\b(?:SELECT|FROM|WHERE|JOIN|LEFT|RIGHT|GROUP|BY|ORDER|LIMIT|OFFSET|INSERT|UPDATE|DELETE|CREATE|ALTER|DROP|WITH|AS|AND|OR|NOT|NULL|TRUE|FALSE|SET|GET|SCAN|HGETALL|ZRANGE|FIND|AGGREGATE|MATCH|PROJECT|SORT|TABLE|INDEX|UNIQUE|PRIMARY|KEY|FOREIGN|REFERENCES|CONSTRAINT|DEFAULT|CHECK|ON|USING|VIEW|SEQUENCE|FUNCTION|PROCEDURE|TRIGGER|RETURNS|BEGIN|END|IF|EXISTS|ENGINE|PARTITION|INTO|VALUES)\b|\{\{[A-Za-z_][A-Za-z0-9_]*\}\}|\b\d+(?:\.\d+)?\b)''',
  caseSensitive: false,
);

/// Syntax colours for query text: comments, strings, keywords, variables and
/// numbers. Presentation only (the editor and the DDL view share it).
TextSpan dbHighlight(BuildContext context, String text, {TextStyle? style}) {
  final theme = Theme.of(context);
  final children = <InlineSpan>[];
  var end = 0;
  for (final match in _highlightTokens.allMatches(text)) {
    if (match.start > end) children.add(TextSpan(text: text.substring(end, match.start)));
    final token = match[0]!;
    final color = token.startsWith('--') || token.startsWith('//') || token.startsWith('/*')
        ? theme.hintColor
        : token.startsWith("'") || token.startsWith('"')
        ? theme.colorScheme.tertiary
        : theme.colorScheme.primary;
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

/// Highlighting is presentation only; native dialect parsing defines statement
/// boundaries, parameter positions and the security classification.
class DbEditorController extends TextEditingController {
  DbEditorController({super.text});
  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    // Keep very large scripts editable without a regexp pass on every frame.
    if (text.length > 131072 || (withComposing && value.composing.isValid && !value.composing.isCollapsed)) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    return dbHighlight(context, text, style: style);
  }

  void insert(String value) {
    final s = selection.isValid ? selection : TextSelection.collapsed(offset: text.length);
    this.value = TextEditingValue(
      text: text.replaceRange(s.start, s.end, value),
      selection: TextSelection.collapsed(offset: s.start + value.length),
    );
  }

  bool _reveal = false;

  /// Adds generated [value] below the text after a blank line, closing the
  /// last SQL statement with `;` first, and puts the caret at the start of
  /// the added text; the editor scrolls to it. Nothing runs.
  void appendStatement(String value, {required bool sql}) {
    var head = text.trimRight();
    if (sql && head.isNotEmpty && !head.endsWith(';')) {
      // A trailing line comment would swallow the `;`.
      head = head.split('\n').last.contains('--') ? '$head\n;' : '$head;';
    }
    final prefix = head.isEmpty ? '' : '$head\n\n';
    _reveal = true;
    this.value = TextEditingValue(
      text: '$prefix${value.trim()}',
      selection: TextSelection.collapsed(offset: prefix.length),
    );
  }

  /// Whether the editor should bring the caret into view (once).
  bool takeReveal() {
    final reveal = _reveal;
    _reveal = false;
    return reveal;
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
  final _scroll = ScrollController();
  final _codeKey = GlobalKey();
  bool _findOpen = false, _busy = false, _measureQueued = false;
  int _generation = 0;

  /// Visible line numbers (number, top, height) and the caret line, both in
  /// the code area's coordinates; measured from the laid-out text so wrapped
  /// lines keep their numbers aligned.
  List<(int, double, double)> _lines = const [];
  (double, double)? _caretLine;
  bool get _current => mounted && widget.isCurrent();

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_queueMeasure);
    _scroll.addListener(_queueMeasure);
    _focus.addListener(_queueMeasure);
    // Text appended while this tab was hidden is revealed once shown.
    _queueMeasure();
  }

  @override
  void didUpdateWidget(DbQueryEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_queueMeasure);
      widget.controller.addListener(_queueMeasure);
    }
  }

  void _queueMeasure() {
    if (_measureQueued || !mounted) return;
    _measureQueued = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _measureQueued = false;
      if (mounted && widget.controller.takeReveal()) {
        _focus.requestFocus();
        _editable()?.bringIntoView(widget.controller.selection.base);
      }
      _measure();
    });
  }

  EditableTextState? _editable() {
    EditableTextState? found;
    void visit(Element element) {
      if (found != null) return;
      if (element is StatefulElement && element.state is EditableTextState) {
        found = element.state as EditableTextState;
        return;
      }
      element.visitChildren(visit);
    }

    _codeKey.currentContext?.visitChildElements(visit);
    return found;
  }

  void _measure() {
    final box = _codeKey.currentContext?.findRenderObject() as RenderBox?;
    final render = _editable()?.renderEditable;
    if (!mounted || box == null || render == null || !box.hasSize || !render.hasSize || !render.attached) return;
    double top(int offset) =>
        box.globalToLocal(render.localToGlobal(render.getLocalRectForCaret(TextPosition(offset: offset)).topLeft)).dy;
    final text = widget.controller.text, lineHeight = render.preferredLineHeight;
    final lines = <(int, double, double)>[];
    var start = 0, number = 1;
    while (number <= 20000) {
      final y = top(start);
      if (y > box.size.height) break;
      if (y + lineHeight >= 0) lines.add((number, y, lineHeight));
      final next = text.indexOf('\n', start);
      if (next < 0) break;
      start = next + 1;
      number++;
    }
    final selection = widget.controller.selection;
    final caret = selection.isValid && _focus.hasFocus
        ? (top(selection.extentOffset.clamp(0, text.length)), lineHeight)
        : null;
    if (!listEquals(lines, _lines) || caret != _caretLine) {
      setState(() {
        _lines = lines;
        _caretLine = caret;
      });
    }
  }

  @override
  void dispose() {
    _generation++;
    widget.controller.removeListener(_queueMeasure);
    _scroll.dispose();
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

  void _error() => showSnack(context, context.l10n.dbOperationFailed, error: true);
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
      if (completions.isEmpty) return;
      // Pop the list up at the caret, like the classic SQL editors.
      final render = _editable()?.renderEditable;
      final caret = render == null
          ? glassAnchorRect(context)
          : MatrixUtils.transformRect(
              render.getTransformTo(null),
              render.getLocalRectForCaret(
                TextPosition(offset: (selection.isValid ? selection.extentOffset : text.length).clamp(0, text.length)),
              ),
            );
      final completion = await showGlassMenu<DbCompletion>(
        context: context,
        anchor: caret,
        minWidth: 280,
        entries: [
          for (final item in completions.take(80))
            GlassMenuItem(value: item, label: item.label, subtitle: item.detail.isEmpty ? null : item.detail),
        ],
      );
      _focus.requestFocus();
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

  static String get _runKeys => AppPlatform.isMacOS ? '⌘↵' : 'Ctrl+Enter'; // l10n-ignore: key names
  static String get _scriptKeys => AppPlatform.isMacOS ? '⌥X' : 'Alt+X'; // l10n-ignore: key names

  /// Like the classic SQL clients: the selection if there is one, otherwise
  /// the statement under the cursor.
  void _runStatement() {
    final selection = widget.controller.selection;
    widget.onRun(selection.isValid && !selection.isCollapsed ? DbRunMode.selection : DbRunMode.current);
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
              tooltip: l.dbRunStatementHelp(_runKeys),
              onPressed: widget.running ? null : _runStatement,
            ),
            const SizedBox(width: GlassSpacing.s4),
            tool(
              'db-run-script',
              Icons.playlist_play_rounded,
              l.dbRunScriptHelp(_scriptKeys),
              widget.running ? null : () => widget.onRun(DbRunMode.script),
            ),
            GlassMenuButton<DbRunMode>(
              key: const ValueKey('db-run-modes'),
              entries: [
                GlassMenuItem(
                  value: DbRunMode.current,
                  label: l.dbRunCurrent,
                  icon: Icons.play_arrow_rounded,
                  shortcut: _runKeys,
                ),
                GlassMenuItem(
                  value: DbRunMode.selection,
                  label: l.dbRunSelection,
                  icon: Icons.highlight_alt_rounded,
                  shortcut: _runKeys,
                ),
                GlassMenuItem(
                  value: DbRunMode.script,
                  label: l.dbRunScript,
                  icon: Icons.playlist_play_rounded,
                  shortcut: _scriptKeys,
                ),
              ],
              onSelected: (mode) {
                if (!widget.running) widget.onRun(mode);
              },
              builder: (context, open) => tool('db-run-menu', Icons.expand_more_rounded, l.dbRun, open),
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

  /// Gutter with line numbers, current-line band and the text itself.
  Widget _codeArea(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final mono = tokens.typography.mono.copyWith(height: 1.5, color: tokens.palette.label);
    final digits = '${_lines.isEmpty ? 1 : _lines.last.$1}'.length.clamp(2, 6);
    final gutter = (mono.fontSize ?? 13) * .62 * digits + GlassSpacing.s16;
    final caret = _caretLine;
    return LayoutBuilder(
      builder: (context, bounds) {
        if (!_measureQueued) _queueMeasure();
        return Stack(
          key: _codeKey,
          children: [
            if (caret != null)
              Positioned(
                left: 0,
                right: 0,
                top: caret.$1,
                height: caret.$2,
                child: ColoredBox(color: tokens.surfaces.fillHover),
              ),
            Positioned(
              left: 0,
              top: 0,
              bottom: 0,
              width: gutter,
              child: DecoratedBox(
                decoration: BoxDecoration(
                  border: Border(right: BorderSide(color: tokens.surfaces.separator)),
                ),
                child: ClipRect(
                  child: Stack(
                    children: [
                      for (final (number, top, height) in _lines)
                        Positioned(
                          right: GlassSpacing.s8,
                          top: top,
                          height: height,
                          child: Center(
                            child: Text(
                              '$number',
                              style: mono.copyWith(
                                color: caret != null && (caret.$1 - top).abs() < 1
                                    ? tokens.palette.label
                                    : tokens.palette.tertiary,
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ),
            Padding(
              padding: EdgeInsets.fromLTRB(gutter + GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
              child: TextField(
                key: const ValueKey('db-query-editor'),
                controller: widget.controller,
                focusNode: _focus,
                scrollController: _scroll,
                expands: true,
                maxLines: null,
                minLines: null,
                decoration: InputDecoration(
                  isCollapsed: true,
                  filled: false,
                  border: InputBorder.none,
                  enabledBorder: InputBorder.none,
                  focusedBorder: InputBorder.none,
                  disabledBorder: InputBorder.none,
                  contentPadding: EdgeInsets.zero,
                  hintText: l.dbEditorHint,
                  hintStyle: mono.copyWith(color: tokens.palette.tertiary),
                ),
                style: mono,
                strutStyle: StrutStyle.fromTextStyle(mono, forceStrutHeight: true),
                cursorColor: tokens.palette.accent,
                cursorWidth: 2,
                cursorRadius: const Radius.circular(1),
                textAlignVertical: TextAlignVertical.top,
                autocorrect: false,
                enableSuggestions: false,
                smartDashesType: SmartDashesType.disabled,
                smartQuotesType: SmartQuotesType.disabled,
              ),
            ),
          ],
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyX, alt: true): () => widget.onRun(DbRunMode.script),
        for (final meta in [false, true]) ...{
          SingleActivator(LogicalKeyboardKey.enter, control: !meta, meta: meta): _runStatement,
          SingleActivator(LogicalKeyboardKey.enter, control: !meta, meta: meta, shift: true): () =>
              widget.onRun(DbRunMode.script),
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
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Align(alignment: Alignment.centerLeft, child: _toolbar(context)),
          Container(height: 1, color: GlassTokens.of(context).surfaces.separator),
          if (_busy) const LinearProgressIndicator(),
          if (_findOpen)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
              child: Wrap(
                spacing: GlassSpacing.s6,
                runSpacing: GlassSpacing.s6,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  SizedBox(
                    width: 200,
                    child: GlassField(controller: _find, search: true, placeholder: l.commonSearch),
                  ),
                  SizedBox(
                    width: 200,
                    child: GlassField(controller: _replace, placeholder: l.dbReplace),
                  ),
                  GlassButton(label: l.dbReplace, size: GlassControlSize.sm, onPressed: () => _replaceText(false)),
                  GlassButton(label: l.dbReplaceAll, size: GlassControlSize.sm, onPressed: () => _replaceText(true)),
                ],
              ),
            ),
          Expanded(child: _codeArea(context)),
        ],
      ),
    );
  }
}
