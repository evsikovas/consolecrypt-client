import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_editor.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Top-level statements of SQL text, each with its `;`. Quotes, dollar
/// quotes and comments never split. Display only: what runs is parsed natively.
List<String> splitSqlStatements(String text) {
  final statements = <String>[];
  var start = 0, i = 0;
  String? dollar;
  void cut(int end) {
    final statement = text.substring(start, end).trim();
    if (statement.isNotEmpty) statements.add(statement);
    start = end;
  }

  while (i < text.length) {
    final c = text[i];
    if (dollar != null) {
      if (text.startsWith(dollar, i)) {
        i += dollar.length;
        dollar = null;
      } else {
        i++;
      }
      continue;
    }
    if (c == "'" || c == '"' || c == '`') {
      i++;
      while (i < text.length) {
        if (text[i] == c) {
          if (i + 1 < text.length && text[i + 1] == c) {
            i += 2;
            continue;
          }
          break;
        }
        i++;
      }
      i++;
      continue;
    }
    if (text.startsWith('--', i)) {
      final end = text.indexOf('\n', i);
      i = end < 0 ? text.length : end + 1;
      continue;
    }
    if (text.startsWith('/*', i)) {
      final end = text.indexOf('*/', i + 2);
      i = end < 0 ? text.length : end + 2;
      continue;
    }
    if (c == r'$') {
      final tag = RegExp(r'\$[A-Za-z_]*\$').matchAsPrefix(text, i);
      if (tag != null) {
        dollar = tag[0];
        i = tag.end;
        continue;
      }
    }
    if (c == ';') cut(i + 1);
    i++;
  }
  cut(text.length);
  return statements;
}

/// Whether database code views wrap long lines (a local setting, on by
/// default).
bool dbCodeWrap(WidgetRef ref) => ref.watch(localSettingsProvider).value?.dbCodeWrap ?? true;

/// "Wrap lines" switch for database code views; remembered locally.
class DbWrapToggle extends ConsumerWidget {
  const DbWrapToggle({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final wrap = dbCodeWrap(ref), l = context.l10n;
    return GlassIconButton(
      key: const ValueKey('db-code-wrap'),
      icon: Icons.wrap_text_rounded,
      tooltip: l.dbWrapLines,
      selected: wrap,
      style: GlassIconButtonStyle.plain,
      size: 26,
      iconSize: 16,
      onPressed: () {
        final settings = ref.read(settingsServiceProvider);
        unawaited(settings.updateLocal(settings.currentLocal.copyWith(dbCodeWrap: !wrap)).catchError((Object _) {}));
      },
    );
  }
}

/// The highlighted text of one block split into lines of (text, style).
List<List<(String, TextStyle?)>> _lines(TextSpan span) {
  final lines = <List<(String, TextStyle?)>>[[]];
  void visit(InlineSpan node, TextStyle? inherited) {
    if (node is! TextSpan) return;
    final style = inherited?.merge(node.style) ?? node.style;
    final text = node.text;
    if (text != null) {
      final parts = text.split('\n');
      for (var i = 0; i < parts.length; i++) {
        if (i > 0) lines.add([]);
        if (parts[i].isNotEmpty) lines.last.add((parts[i], style));
      }
    }
    for (final child in node.children ?? const <InlineSpan>[]) {
      visit(child, style);
    }
  }

  visit(span, null);
  return lines;
}

TextSpan _span(List<(String, TextStyle?)> parts) => TextSpan(
  children: [for (final (text, style) in parts) TextSpan(text: text, style: style)],
);

/// One wrapped source line: the first visual line at full width, the rest
/// indented by [indent] so continuations stand out.
class _WrappedLine extends StatelessWidget {
  const _WrappedLine({required this.parts, required this.style, required this.strut, required this.indent});
  final List<(String, TextStyle?)> parts;
  final TextStyle style;
  final StrutStyle strut;
  final double indent;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final whole = TextSpan(style: style, children: [_span(parts)]);
      final painter = TextPainter(
        text: whole,
        textDirection: TextDirection.ltr,
        strutStyle: strut,
        textScaler: MediaQuery.textScalerOf(context),
      )..layout(maxWidth: constraints.maxWidth);
      final multiline = painter.computeLineMetrics().length > 1;
      final end = multiline ? painter.getLineBoundary(const TextPosition(offset: 0)).end : 0;
      painter.dispose();
      if (!multiline || end <= 0) return Text.rich(whole, strutStyle: strut);
      final first = <(String, TextStyle?)>[], rest = <(String, TextStyle?)>[];
      var offset = 0;
      for (final (text, partStyle) in parts) {
        final cut = (end - offset).clamp(0, text.length);
        if (cut > 0) first.add((text.substring(0, cut), partStyle));
        if (cut < text.length) rest.add((text.substring(cut), partStyle));
        offset += text.length;
      }
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text.rich(
            TextSpan(style: style, children: [_span(first)]),
            strutStyle: strut,
            softWrap: false,
          ),
          Padding(
            padding: EdgeInsets.only(left: indent),
            child: Text.rich(
              TextSpan(style: style, children: [_span(rest)]),
              strutStyle: strut,
            ),
          ),
        ],
      );
    },
  );
}

/// Read-only code with line numbers and syntax colours; [blocks] are
/// separated by a divider. With [wrap], long lines wrap with indented
/// continuations and the number stands on the first visual line; without
/// it the whole code scrolls sideways under a visible scrollbar.
class DbCodeView extends StatefulWidget {
  const DbCodeView({required this.blocks, this.wrap = true, super.key});
  final List<String> blocks;
  final bool wrap;

  @override
  State<DbCodeView> createState() => _DbCodeViewState();
}

class _DbCodeViewState extends State<DbCodeView> {
  final _vertical = ScrollController(), _horizontal = ScrollController();

  @override
  void dispose() {
    _vertical.dispose();
    _horizontal.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final blocks = widget.blocks;
    final mono = tokens.typography.mono.copyWith(height: 1.5, color: tokens.palette.label);
    final strut = StrutStyle.fromTextStyle(mono, forceStrutHeight: true);
    final fontSize = mono.fontSize ?? 13;
    final lineHeight = fontSize * 1.5;
    final lines = [for (final block in blocks) '\n'.allMatches(block).length + 1];
    final total = lines.fold<int>(0, (sum, n) => sum + n);
    final gutter = fontSize * .62 * '$total'.length.clamp(2, 6) + GlassSpacing.s16;
    final numberStyle = mono.copyWith(color: tokens.palette.tertiary);
    Widget divider() => SizedBox(
      height: lineHeight,
      child: Center(child: Container(height: 1, color: tokens.surfaces.separator)),
    );
    final gutterLine = Positioned(
      left: gutter - 1,
      top: 0,
      bottom: 0,
      width: 1,
      child: ColoredBox(color: tokens.surfaces.separator),
    );
    final Widget body;
    if (widget.wrap) {
      var number = 1;
      final children = <Widget>[];
      for (var b = 0; b < blocks.length; b++) {
        if (b > 0) children.add(divider());
        final rows = <Widget>[];
        for (final line in _lines(dbHighlight(context, blocks[b], style: mono))) {
          rows.add(
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SizedBox(
                  width: gutter,
                  child: Padding(
                    padding: const EdgeInsets.only(right: GlassSpacing.s8),
                    child: Text('${number++}', textAlign: TextAlign.right, strutStyle: strut, style: numberStyle),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s8),
                Expanded(
                  child: _WrappedLine(parts: line, style: mono, strut: strut, indent: fontSize * .62 * 4),
                ),
              ],
            ),
          );
        }
        children.add(
          Column(key: ValueKey('db-code-block-$b'), crossAxisAlignment: CrossAxisAlignment.stretch, children: rows),
        );
      }
      body = Stack(
        children: [
          gutterLine,
          SelectionArea(
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: children),
          ),
        ],
      );
    } else {
      var first = 1;
      final numbers = <Widget>[];
      for (var b = 0; b < blocks.length; b++) {
        if (b > 0) numbers.add(SizedBox(height: lineHeight));
        numbers.add(
          Text(
            [for (var n = first; n < first + lines[b]; n++) '$n'].join('\n'),
            textAlign: TextAlign.right,
            softWrap: false,
            strutStyle: strut,
            style: numberStyle,
          ),
        );
        first += lines[b];
      }
      body = Stack(
        children: [
          gutterLine,
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Container(
                width: gutter,
                padding: const EdgeInsets.only(right: GlassSpacing.s8),
                child: Column(crossAxisAlignment: CrossAxisAlignment.end, children: numbers),
              ),
              Expanded(
                child: Scrollbar(
                  controller: _horizontal,
                  thumbVisibility: true,
                  child: SingleChildScrollView(
                    key: const ValueKey('db-code-horizontal'),
                    controller: _horizontal,
                    scrollDirection: Axis.horizontal,
                    padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, 0, GlassSpacing.s8, GlassSpacing.s12),
                    child: SelectionArea(
                      child: IntrinsicWidth(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            for (var b = 0; b < blocks.length; b++) ...[
                              if (b > 0) divider(),
                              Text.rich(
                                dbHighlight(context, blocks[b], style: mono),
                                key: ValueKey('db-code-block-$b'),
                                softWrap: false,
                                strutStyle: strut,
                              ),
                            ],
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ],
      );
    }
    return Scrollbar(
      controller: _vertical,
      child: SingleChildScrollView(key: const ValueKey('db-code-vertical'), controller: _vertical, child: body),
    );
  }
}

/// [DbCodeView] on a rounded field surface with the wrap switch, at most
/// [maxHeight] tall (a reviewed statement, a JSON description).
class DbCodeBlock extends ConsumerWidget {
  const DbCodeBlock({required this.text, super.key, this.maxHeight = 320});
  final String text;
  final double maxHeight;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final radius = BorderRadius.circular(tokens.radii.card);
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxHeight),
      child: DecoratedBox(
        decoration: ShapeDecoration(
          color: tokens.surfaces.fillField,
          shape: RoundedRectangleBorder(
            borderRadius: radius,
            side: BorderSide(color: tokens.surfaces.hairlineCard),
          ),
        ),
        child: ClipRRect(
          borderRadius: radius,
          child: Stack(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(0, GlassSpacing.s8, GlassSpacing.s24, GlassSpacing.s8),
                child: DbCodeView(blocks: [text], wrap: dbCodeWrap(ref)),
              ),
              const Positioned(top: 2, right: 2, child: DbWrapToggle()),
            ],
          ),
        ),
      ),
    );
  }
}

/// DDL of a structure object as a code block: copy it or open it in a new
/// query tab (closes with true). Wiped and closed when the vault scope ends.
class DbDdlDialog extends ConsumerStatefulWidget {
  const DbDdlDialog({required this.title, required this.value, required this.engine, super.key});
  final String title;
  final SecretText value;
  final DbEngine engine;
  @override
  ConsumerState<DbDdlDialog> createState() => _DbDdlDialogState();
}

class _DbDdlDialogState extends ConsumerState<DbDdlDialog> {
  late final _scope = ref.read(dbScopeProvider);

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        widget.value.wipe();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider)) || widget.value.isWiped) {
      return const SizedBox.shrink();
    }
    final l = context.l10n, text = widget.value.expose(), window = MediaQuery.sizeOf(context);
    final blocks = widget.engine.sql ? splitSqlStatements(text) : [text];
    return GlassDialog(
      key: const ValueKey('db-ddl-dialog'),
      title: widget.title,
      // Up to 90% of the window (at most 1100 px); as tall as the code, up
      // to 85% of the window, then it scrolls.
      width: (window.width * .9).clamp(320.0, 1100.0),
      content: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: (window.height * .85 - 140).clamp(120.0, double.infinity)),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Align(alignment: Alignment.centerRight, child: DbWrapToggle()),
            Flexible(
              child: DbCodeView(blocks: blocks.isEmpty ? [text] : blocks, wrap: dbCodeWrap(ref)),
            ),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonClose, onPressed: () => closeDialog<bool>(context, false)),
        GlassButton(
          key: const ValueKey('db-ddl-copy'),
          label: l.commonCopy,
          icon: Icons.content_copy_rounded,
          onPressed: () => copyPlainWithNotice(context, ref, blocks.join('\n\n'), what: l.copyWhatDdl),
        ),
      ],
      primaryAction: GlassButton(
        key: const ValueKey('db-ddl-open'),
        label: l.dbOpenInEditor,
        icon: Icons.edit_note_rounded,
        style: GlassButtonStyle.prominent,
        onPressed: () => closeDialog<bool>(context, true),
      ),
    );
  }
}
