import 'package:consolecrypt/core/glass/glass.dart';
import 'package:material_ui/material_ui.dart';

/// A block of an AI answer between code fences.
sealed class MdBlock {
  const MdBlock();
}

final class MdHeading extends MdBlock {
  const MdHeading(this.level, this.text);
  final int level;
  final String text;
}

final class MdParagraph extends MdBlock {
  const MdParagraph(this.text);
  final String text;
}

/// A list item; [marker] is "•" or the original number ("2.").
final class MdListItem extends MdBlock {
  const MdListItem(this.marker, this.text, this.depth);
  final String marker, text;
  final int depth;
}

final class MdQuote extends MdBlock {
  const MdQuote(this.text);
  final String text;
}

final class MdRule extends MdBlock {
  const MdRule();
}

final _heading = RegExp(r'^(#{1,6})\s+(.*?)\s*#*\s*$');
final _bullet = RegExp(r'^(\s*)[-*+]\s+(.*)$');
final _numbered = RegExp(r'^(\s*)(\d{1,3}[.)])\s+(.*)$');
final _rule = RegExp(r'^\s*([-*_])(\s*\1){2,}\s*$');

/// Line-based Markdown subset used by chat models: headings, lists, quotes,
/// rules and paragraphs (soft line breaks kept). Code fences are split off
/// before (see `parseAnswer`).
List<MdBlock> parseMarkdownBlocks(String text) {
  final blocks = <MdBlock>[];
  final paragraph = <String>[];
  void flush() {
    if (paragraph.isEmpty) return;
    blocks.add(MdParagraph(paragraph.join('\n')));
    paragraph.clear();
  }

  for (final line in text.split('\n')) {
    if (line.trim().isEmpty) {
      flush();
      continue;
    }
    if (_rule.hasMatch(line)) {
      flush();
      blocks.add(const MdRule());
    } else if (_heading.firstMatch(line) case final m?) {
      flush();
      blocks.add(MdHeading(m.group(1)!.length, m.group(2)!));
    } else if (_bullet.firstMatch(line) case final m?) {
      flush();
      blocks.add(MdListItem('•', m.group(2)!, (m.group(1)!.length ~/ 2).clamp(0, 4)));
    } else if (_numbered.firstMatch(line) case final m?) {
      flush();
      blocks.add(MdListItem(m.group(2)!, m.group(3)!, (m.group(1)!.length ~/ 2).clamp(0, 4)));
    } else if (line.trimLeft().startsWith('>')) {
      flush();
      blocks.add(MdQuote(line.trimLeft().substring(1).trimLeft()));
    } else {
      paragraph.add(line.trimRight());
    }
  }
  flush();
  return blocks;
}

/// Inline spans: `code`, **bold**, __bold__, *italic*, [text](url) (the
/// link is shown as text only; nothing is opened from an answer).
final _inline = RegExp(
  r'`([^`\n]+)`|\*\*(.+?)\*\*|__(.+?)__|(?<![\w*])\*(?!\s)([^*\n]+?)\*(?!\w)|\[([^\]\n]+)\]\([^)\s]+\)',
);

List<InlineSpan> markdownInline(String text, TextStyle base, GlassTokens tokens) {
  final spans = <InlineSpan>[];
  var start = 0;
  for (final m in _inline.allMatches(text)) {
    if (m.start > start) spans.add(TextSpan(text: text.substring(start, m.start)));
    if (m.group(1) case final code?) {
      spans.add(
        TextSpan(
          text: code,
          style: tokens.typography.mono.copyWith(
            fontSize: (base.fontSize ?? 14) * .92,
            color: tokens.palette.label,
            backgroundColor: tokens.surfaces.inset,
          ),
        ),
      );
    } else if (m.group(2) ?? m.group(3) case final bold?) {
      spans.add(
        TextSpan(
          children: markdownInline(bold, base, tokens),
          style: const TextStyle(fontWeight: FontWeight.w700),
        ),
      );
    } else if (m.group(4) case final italic?) {
      spans.add(
        TextSpan(
          text: italic,
          style: const TextStyle(fontStyle: FontStyle.italic),
        ),
      );
    } else if (m.group(5) case final link?) {
      spans.add(
        TextSpan(
          text: link,
          style: TextStyle(color: tokens.palette.accent, decoration: TextDecoration.underline),
        ),
      );
    }
    start = m.end;
  }
  if (start < text.length) spans.add(TextSpan(text: text.substring(start)));
  return spans;
}

/// Renders Markdown text of an AI answer (no code fences) in app styles.
class MarkdownText extends StatelessWidget {
  const MarkdownText(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final body = DefaultTextStyle.of(context).style.merge(tokens.typography.body.copyWith(color: tokens.palette.label));
    Widget rich(String text, TextStyle style) =>
        Text.rich(TextSpan(style: style, children: markdownInline(text, style, tokens)));
    final blocks = parseMarkdownBlocks(text);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final (i, block) in blocks.indexed)
          Padding(
            padding: EdgeInsets.only(
              top: i == 0
                  ? 0
                  : block is MdHeading
                  ? GlassSpacing.s12
                  : block is MdListItem && blocks[i - 1] is MdListItem
                  ? 2
                  : GlassSpacing.s6,
            ),
            child: switch (block) {
              MdHeading(:final level, :final text) => rich(
                text,
                (level <= 1
                        ? tokens.typography.title2
                        : level == 2
                        ? tokens.typography.title3
                        : tokens.typography.bodyEmph)
                    .copyWith(color: tokens.palette.label, fontWeight: FontWeight.w700),
              ),
              MdParagraph(:final text) => rich(text, body),
              MdListItem(:final marker, :final text, :final depth) => Padding(
                padding: EdgeInsets.only(left: depth * 16.0),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(
                      width: marker == '•' ? 16 : 24,
                      child: Text(marker, style: body.copyWith(color: tokens.secondaryLabel)),
                    ),
                    Expanded(child: rich(text, body)),
                  ],
                ),
              ),
              MdQuote(:final text) => Container(
                padding: const EdgeInsets.only(left: GlassSpacing.s8),
                decoration: BoxDecoration(
                  border: Border(left: BorderSide(color: tokens.surfaces.separator, width: 3)),
                ),
                child: rich(text, body.copyWith(color: tokens.secondaryLabel)),
              ),
              MdRule() => Divider(height: 1, color: tokens.surfaces.separator),
            },
          ),
      ],
    );
  }
}
