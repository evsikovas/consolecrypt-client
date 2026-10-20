import 'package:material_ui/material_ui.dart';

/// Lightweight lexical coloring only. Parsing and schema validation stay native.
/// Large manifests use plain text to keep editing responsive.
class KubeYamlController extends TextEditingController {
  KubeYamlController({super.text});
  @override
  TextSpan buildTextSpan({required BuildContext context, TextStyle? style, required bool withComposing}) {
    if (text.length > 200000 || (withComposing && value.isComposingRangeValid && !value.composing.isCollapsed)) {
      return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    }
    final colors = Theme.of(context).colorScheme;
    final spans = <InlineSpan>[];
    final token = RegExp(
      r'''(^\s*(?:-\s+)?[\w./-]+\s*:)|("(?:[^"\\]|\\.)*"|'(?:[^']|'')*')|(^\s*#.*$)|\b(true|false|null|[0-9]+(?:\.[0-9]+)?)\b''',
      multiLine: true,
    );
    var offset = 0;
    for (final match in token.allMatches(text)) {
      if (offset < match.start) spans.add(TextSpan(text: text.substring(offset, match.start)));
      final color = match.group(1) != null
          ? colors.primary
          : match.group(3) != null
          ? colors.onSurfaceVariant
          : colors.tertiary;
      spans.add(
        TextSpan(
          text: match.group(0),
          style: TextStyle(color: color),
        ),
      );
      offset = match.end;
    }
    if (offset < text.length) spans.add(TextSpan(text: text.substring(offset)));
    return TextSpan(style: style, children: spans);
  }
}
