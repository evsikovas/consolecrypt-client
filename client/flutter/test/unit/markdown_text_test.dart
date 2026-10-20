import 'package:consolecrypt/ai/markdown_text.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('blocks: headings, lists, quotes, rules and paragraphs', () {
    final blocks = parseMarkdownBlocks(
      '## Что проверить\n1. **Логи** пустые\n2. Лимит `1Gi`\n- пункт\n  - вложенный\n> цитата\n---\nобычный текст\nвторая строка',
    );
    expect(blocks[0], isA<MdHeading>().having((h) => h.level, 'level', 2).having((h) => h.text, 'text', 'Что проверить'));
    expect(blocks[1], isA<MdListItem>().having((i) => i.marker, 'marker', '1.'));
    expect(blocks[2], isA<MdListItem>().having((i) => i.text, 'text', 'Лимит `1Gi`'));
    expect(blocks[3], isA<MdListItem>().having((i) => i.marker, 'marker', '•'));
    expect(blocks[4], isA<MdListItem>().having((i) => i.depth, 'depth', 1));
    expect(blocks[5], isA<MdQuote>());
    expect(blocks[6], isA<MdRule>());
    expect(blocks[7], isA<MdParagraph>().having((p) => p.text, 'text', 'обычный текст\nвторая строка'));
  });

  test('snake_case and multiplication are not italic', () {
    expect(parseMarkdownBlocks('shared_buffers = 2 * 64'), [isA<MdParagraph>()]);
  });
}
