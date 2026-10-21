import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Longest text that still gets coloured (larger files stay plain so
/// scrolling and typing remain smooth).
const _highlightLimit = 200000;

/// Colours of the lexical YAML highlighting.
final class DockerYamlStyles {
  const DockerYamlStyles({required this.key, required this.string, required this.comment, required this.scalar});
  final TextStyle key, string, comment, scalar;

  factory DockerYamlStyles.of(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    return DockerYamlStyles(
      key: TextStyle(color: p.accent),
      string: TextStyle(color: p.success),
      comment: TextStyle(color: tokens.secondaryLabel, fontStyle: FontStyle.italic),
      scalar: TextStyle(color: p.warning),
    );
  }
}

final _key = RegExp(r'''^(\s*(?:-\s+)?)("[^"]*"|'[^']*'|[^\s#'"{}\[\],&*!|>%@`][^:#]*?)(:)(?=\s|$)''');
final _quoted = RegExp(r'''"(?:[^"\\]|\\.)*"|'(?:[^']|'')*\'?''');
final _scalar = RegExp(r'^(?:true|false|null|yes|no|on|off|~|-?\d+(?:\.\d+)?)$', caseSensitive: false);

/// Where a comment starts on [line] (outside quotes), or -1.
int _commentStart(String line) {
  String? quote;
  for (var i = 0; i < line.length; i++) {
    final c = line[i];
    if (quote != null) {
      if (c == quote && (quote == "'" || i == 0 || line[i - 1] != r'\')) quote = null;
      continue;
    }
    if (c == '"' || c == "'") {
      quote = c;
    } else if (c == '#' && (i == 0 || line[i - 1] == ' ' || line[i - 1] == '\t')) {
      return i;
    }
  }
  return -1;
}

void _value(String value, List<InlineSpan> out, DockerYamlStyles styles) {
  if (value.isEmpty) return;
  final trimmed = value.trim();
  if (_scalar.hasMatch(trimmed)) {
    final start = value.indexOf(trimmed);
    if (start > 0) out.add(TextSpan(text: value.substring(0, start)));
    out.add(TextSpan(text: trimmed, style: styles.scalar));
    final end = start + trimmed.length;
    if (end < value.length) out.add(TextSpan(text: value.substring(end)));
    return;
  }
  var offset = 0;
  for (final match in _quoted.allMatches(value)) {
    if (match.start > offset) out.add(TextSpan(text: value.substring(offset, match.start)));
    out.add(TextSpan(text: match.group(0), style: styles.string));
    offset = match.end;
  }
  if (offset < value.length) out.add(TextSpan(text: value.substring(offset)));
}

/// Keys, quoted strings, scalars and comments of one YAML line.
List<InlineSpan> dockerYamlLineSpans(String line, DockerYamlStyles styles) {
  final out = <InlineSpan>[];
  final comment = _commentStart(line);
  final code = comment < 0 ? line : line.substring(0, comment);
  var rest = code;
  final key = _key.firstMatch(code);
  if (key != null) {
    if (key.group(1)!.isNotEmpty) out.add(TextSpan(text: key.group(1)));
    out.add(TextSpan(text: key.group(2), style: styles.key));
    out.add(TextSpan(text: key.group(3)));
    rest = code.substring(key.end);
  }
  _value(rest, out, styles);
  if (comment >= 0) out.add(TextSpan(text: line.substring(comment), style: styles.comment));
  return out;
}

/// The whole text as coloured spans (plain above the size limit).
TextSpan dockerYamlSpan(String text, DockerYamlStyles styles, TextStyle? style) {
  if (text.length > _highlightLimit) return TextSpan(text: text, style: style);
  final lines = text.split('\n');
  final children = <InlineSpan>[];
  for (var i = 0; i < lines.length; i++) {
    children.addAll(dockerYamlLineSpans(lines[i], styles));
    if (i < lines.length - 1) children.add(const TextSpan(text: '\n'));
  }
  return TextSpan(style: style, children: children);
}

/// Text field controller that colours YAML as it is typed.
class DockerYamlController extends TextEditingController {
  DockerYamlController({super.text});

  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    if (withComposing && value.isComposingRangeValid && !value.composing.isCollapsed) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    return dockerYamlSpan(text, DockerYamlStyles.of(context), style);
  }
}

TextStyle _codeStyle(BuildContext context) {
  final tokens = GlassTokens.of(context);
  return tokens.typography.mono.copyWith(fontSize: 12, height: 1.5, color: tokens.palette.label);
}

StrutStyle _strut(TextStyle style) => StrutStyle.fromTextStyle(style, forceStrutHeight: true);

/// Line numbers matching the code's line height.
class _Gutter extends StatelessWidget {
  const _Gutter({required this.lines, required this.style});
  final int lines;
  final TextStyle style;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final digits = math.max(2, '$lines'.length);
    return ExcludeSemantics(
      child: Container(
        padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s8),
        decoration: BoxDecoration(
          border: Border(right: BorderSide(color: tokens.surfaces.separator)),
        ),
        child: Text(
          [for (var i = 1; i <= lines; i++) '$i'.padLeft(digits)].join('\n'),
          textAlign: TextAlign.right,
          strutStyle: _strut(style),
          style: style.copyWith(color: tokens.palette.tertiary),
        ),
      ),
    );
  }
}

/// Frame shared by the viewer and the editor: a code card that scrolls both
/// ways with the gutter fixed to the text.
class _CodeFrame extends StatefulWidget {
  const _CodeFrame({required this.lines, required this.builder, this.contentWidth});
  final int lines;

  /// Width the code needs (the editor measures it); null sizes to the text.
  final double? contentWidth;
  final Widget Function(BuildContext context, TextStyle style) builder;

  @override
  State<_CodeFrame> createState() => _CodeFrameState();
}

class _CodeFrameState extends State<_CodeFrame> {
  final _vertical = ScrollController();
  final _horizontal = ScrollController();

  @override
  void dispose() {
    _vertical.dispose();
    _horizontal.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final style = _codeStyle(context);
    return DecoratedBox(
      decoration: ShapeDecoration(
        color: tokens.surfaces.fillField,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(tokens.radii.card),
          side: BorderSide(color: tokens.surfaces.hairlineCard),
        ),
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(tokens.radii.card),
        child: Scrollbar(
          controller: _vertical,
          child: SingleChildScrollView(
            controller: _vertical,
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                _Gutter(lines: widget.lines, style: style),
                Expanded(
                  child: Scrollbar(
                    controller: _horizontal,
                    child: SingleChildScrollView(
                      controller: _horizontal,
                      scrollDirection: Axis.horizontal,
                      child: Padding(
                        padding: const EdgeInsets.all(GlassSpacing.s8),
                        child: widget.contentWidth == null
                            ? widget.builder(context, style)
                            : SizedBox(width: widget.contentWidth, child: widget.builder(context, style)),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Read-only YAML with line numbers and colouring; selectable.
class DockerYamlView extends StatelessWidget {
  const DockerYamlView({required this.text, super.key});
  final String text;

  @override
  Widget build(BuildContext context) {
    final styles = DockerYamlStyles.of(context);
    return _CodeFrame(
      lines: '\n'.allMatches(text).length + 1,
      builder: (context, style) => SelectableText.rich(
        dockerYamlSpan(text, styles, style),
        key: const ValueKey('docker-compose-text'),
        strutStyle: _strut(style),
      ),
    );
  }
}

/// Editable YAML: no soft wrap, so line numbers stay aligned.
class DockerYamlEditor extends StatelessWidget {
  const DockerYamlEditor({required this.controller, this.readOnly = false, this.autofocus = false, super.key});
  final DockerYamlController controller;
  final bool readOnly, autofocus;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<TextEditingValue>(
    valueListenable: controller,
    builder: (context, value, _) {
      final style = _codeStyle(context);
      final lines = value.text.split('\n');
      final longest = lines.fold<int>(0, (m, line) => math.max(m, line.length));
      final painter = TextPainter(
        text: TextSpan(text: 'M', style: style),
        textDirection: TextDirection.ltr,
        textScaler: MediaQuery.textScalerOf(context),
      )..layout();
      final width = painter.width * (longest + 2) + 24;
      painter.dispose();
      return LayoutBuilder(
        builder: (context, constraints) => _CodeFrame(
          lines: lines.length,
          contentWidth: math.max(width, math.max(0, constraints.maxWidth - 80)),
          builder: (context, style) => TextField(
            key: const ValueKey('docker-compose-editor'),
            controller: controller,
            readOnly: readOnly,
            autofocus: autofocus,
            maxLines: null,
            style: style,
            strutStyle: _strut(style),
            autocorrect: false,
            enableSuggestions: false,
            enableIMEPersonalizedLearning: false,
            keyboardType: TextInputType.multiline,
            inputFormatters: [LengthLimitingTextInputFormatter(1024 * 1024)],
            decoration: const InputDecoration.collapsed(hintText: null),
          ),
        ),
      );
    },
  );
}
