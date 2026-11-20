/// Lexical view metadata only. Never parses, validates or executes Ansible.
class AnsibleSourceLine {
  const AnsibleSourceLine(this.number, this.start, this.contentEnd, this.end, this.text);
  final int number, start, contentEnd, end;
  final String text;
  int get indent => text.length - text.trimLeft().length;
}

List<AnsibleSourceLine> ansibleSourceLines(String text) {
  final result = <AnsibleSourceLine>[];
  var start = 0;
  for (var i = 0; i <= text.length; i++) {
    if (i != text.length && text.codeUnitAt(i) != 10) continue;
    final contentEnd = i > start && text.codeUnitAt(i - 1) == 13 ? i - 1 : i;
    result.add(
      AnsibleSourceLine(
        result.length + 1,
        start,
        contentEnd,
        i < text.length ? i + 1 : i,
        text.substring(start, contentEnd),
      ),
    );
    start = i + 1;
  }
  return result;
}

class AnsibleFoldRegion {
  const AnsibleFoldRegion({
    required this.headerLine,
    required this.headerOffset,
    required this.headerEnd,
    required this.hiddenStart,
    required this.hiddenEnd,
    required this.endLine,
    required this.indent,
    required this.task,
  });
  final int headerLine, headerOffset, headerEnd, hiddenStart, hiddenEnd, endLine, indent;
  final bool task;
  int get hiddenLines => endLine - headerLine;
}

class _OpenFold {
  _OpenFold(this.line, this.task);
  final AnsibleSourceLine line;
  final bool task;
  bool child = false;
}

/// Indentation stack is capped, so unfinished/hostile YAML remains bounded.
/// Literal scalar bodies cannot manufacture fake task headers.
List<AnsibleFoldRegion> ansibleFoldRegions(String text) {
  if (text.length > 500000) return const [];
  final lines = ansibleSourceLines(text);
  if (lines.length > 20000) return const [];
  final result = <AnsibleFoldRegion>[], stack = <_OpenFold>[];
  int? scalarIndent;
  void finish(_OpenFold open, int end) {
    if (!open.child || end <= open.line.number) return;
    result.add(
      AnsibleFoldRegion(
        headerLine: open.line.number,
        headerOffset: open.line.start,
        headerEnd: open.line.end,
        hiddenStart: open.line.end,
        hiddenEnd: lines[end - 1].end,
        endLine: end,
        indent: open.line.indent,
        task: open.task,
      ),
    );
  }

  for (final line in lines) {
    final trim = line.text.trim();
    if (trim.isEmpty || trim.startsWith('#')) continue;
    if (scalarIndent != null && line.indent > scalarIndent) {
      for (final open in stack) {
        if (line.indent > open.line.indent) open.child = true;
      }
      continue;
    }
    scalarIndent = null;
    while (stack.isNotEmpty && line.indent <= stack.last.line.indent) {
      finish(stack.removeLast(), line.number - 1);
    }
    for (final open in stack) {
      if (line.indent > open.line.indent) open.child = true;
    }
    if (line.text.substring(0, line.indent).contains('\t')) continue;
    final sequence = RegExp(r'^-\s+(?:[A-Za-z_][\w.]*|["\x27].*?["\x27])\s*:').hasMatch(trim);
    final mapping = RegExp(r'^(?:-\s+)?[A-Za-z_][\w.]*\s*:\s*(?:[|>][+\-0-9]*\s*)?(?:#.*)?$').hasMatch(trim);
    final scalar = RegExp(r':\s*[|>][+\-0-9]*\s*(?:#.*)?$').hasMatch(trim);
    if ((sequence || mapping) && stack.length < 64) stack.add(_OpenFold(line, sequence));
    if (scalar) scalarIndent = line.indent;
  }
  var end = lines.length;
  if (lines.last.text.isEmpty && end > 1) end--;
  while (stack.isNotEmpty) {
    finish(stack.removeLast(), end);
  }
  result.sort((a, b) => a.headerOffset.compareTo(b.headerOffset));
  return result;
}

class AnsibleProjectionSegment {
  const AnsibleProjectionSegment(this.start, this.end, this.sourceStart, this.sourceEnd, [this.fold]);
  final int start, end, sourceStart, sourceEnd;
  final AnsibleFoldRegion? fold;
}

/// Synthetic comments represent hidden ranges; they never enter source.text.
class AnsibleProjection {
  AnsibleProjection(this.source, List<AnsibleFoldRegion> regions, Set<int> collapsed) {
    final out = StringBuffer();
    var cursor = 0;
    void append(int start, int end) {
      if (start == end) return;
      final offset = out.length;
      out.write(source.substring(start, end));
      segments.add(AnsibleProjectionSegment(offset, out.length, start, end));
    }

    for (final region in regions) {
      if (!collapsed.contains(region.headerOffset) || region.hiddenStart < cursor) continue;
      append(cursor, region.hiddenStart);
      final start = out.length;
      out.write('${' ' * (region.indent + 2)}# … ${region.hiddenLines}');
      if (source.substring(region.hiddenStart, region.hiddenEnd).endsWith('\n')) out.write('\n');
      segments.add(AnsibleProjectionSegment(start, out.length, region.hiddenStart, region.hiddenEnd, region));
      cursor = region.hiddenEnd;
    }
    append(cursor, source.length);
    text = out.toString();
    sourceLines = ansibleSourceLines(source);
    for (final line in ansibleSourceLines(text)) {
      final marker = markerAt(line.start);
      lineNumbers.add(marker == null ? sourceLine(toSource(line.start, downstream: true)) : null);
    }
  }
  final String source;
  late final String text;
  late final List<AnsibleSourceLine> sourceLines;
  final segments = <AnsibleProjectionSegment>[];
  final lineNumbers = <int?>[];
  AnsibleProjectionSegment? markerAt(int offset) {
    for (final segment in segments) {
      if (segment.fold != null && offset >= segment.start && offset < segment.end) return segment;
    }
    return null;
  }

  int sourceLine(int offset) {
    var low = 0, high = sourceLines.length;
    while (low < high) {
      final mid = (low + high) ~/ 2;
      if (sourceLines[mid].start <= offset) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low.clamp(1, sourceLines.length);
  }

  int toSource(int offset, {bool downstream = false}) {
    offset = offset.clamp(0, text.length);
    for (final segment in segments) {
      if (offset < segment.start || offset > segment.end) continue;
      if (segment.fold == null) return segment.sourceStart + offset - segment.start;
      if (offset == segment.end) return segment.sourceEnd;
      return downstream ? segment.sourceEnd : segment.sourceStart;
    }
    return source.length;
  }

  int toVisible(int offset) {
    offset = offset.clamp(0, source.length);
    for (final segment in segments) {
      if (offset < segment.sourceStart || offset > segment.sourceEnd) continue;
      if (segment.fold == null) return segment.start + offset - segment.sourceStart;
      return offset == segment.sourceEnd ? segment.end : segment.start;
    }
    return text.length;
  }
}
