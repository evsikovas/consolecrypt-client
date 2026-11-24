import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

class TerminalFindButton extends ConsumerWidget {
  const TerminalFindButton({required this.tab, super.key});
  final TerminalTab tab;
  @override
  Widget build(BuildContext context, WidgetRef ref) => GlassIconButton(
    key: const ValueKey('terminal-find-button'),
    icon: Icons.search,
    tooltip: context.l10n.terminalFindText,
    selected: tab.findOpen,
    style: GlassIconButtonStyle.plain,
    size: GlassSizes.tab,
    iconSize: GlassSizes.iconRow,
    onPressed: () => ref.read(terminalTabsProvider.notifier).showFind(tab, !tab.findOpen),
  );
}

typedef TerminalMatch = ({CellOffset start, CellOffset end});

/// Scan logical lines, translating UTF-16 matches back to terminal cells. A
/// wide glyph occupies two cells and a soft wrap does not introduce a newline.
List<TerminalMatch> findTerminalText(Terminal terminal, String query) {
  if (query.isEmpty) return [];
  final pattern = RegExp(RegExp.escape(query), caseSensitive: false, unicode: true);
  final lines = terminal.buffer.lines, hits = <TerminalMatch>[];
  for (var y = 0; y < lines.length;) {
    final firstRow = y, text = StringBuffer();
    do {
      final line = lines[y], length = line.getTrimmedLength();
      for (var x = 0; x < length; x += line.getWidth(x).clamp(1, 2)) {
        final cp = line.getCodePoint(x);
        text.writeCharCode(cp == 0 ? 32 : cp);
      }
      y++;
    } while (y < lines.length && lines[y].isWrapped);

    final matches = pattern.allMatches(text.toString()).iterator;
    if (!matches.moveNext()) continue;
    var units = 0;
    CellOffset? start;
    scan:
    for (var row = firstRow; row < y; row++) {
      final line = lines[row], length = line.getTrimmedLength();
      for (var x = 0; x < length;) {
        final width = line.getWidth(x).clamp(1, 2);
        if (units == matches.current.start) start = CellOffset(x, row);
        units += line.getCodePoint(x) > 0xffff ? 2 : 1;
        x += width;
        if (units == matches.current.end && start != null) {
          hits.add((start: start, end: CellOffset(x, row)));
          if (!matches.moveNext()) break scan;
        }
      }
    }
  }
  return hits;
}

class TerminalFindBar extends StatefulWidget {
  const TerminalFindBar({required this.tab, required this.reveal, required this.close, super.key});
  final TerminalTab tab;
  final ValueChanged<int> reveal;
  final VoidCallback close;
  @override
  State<TerminalFindBar> createState() => _TerminalFindBarState();
}

class _TerminalFindBarState extends State<TerminalFindBar> {
  final _field = FocusNode();
  late final Terminal _terminal = widget.tab.terminal;
  String _query = '';
  List<TerminalMatch> _hits = [];
  int _index = 0;
  TerminalHighlight? _highlight;
  Timer? _refresh;
  @override
  void initState() {
    super.initState();
    _terminal.addListener(_output);
  }

  void _output() {
    if (_query.isEmpty || _refresh?.isActive == true) return;
    _refresh = Timer(const Duration(milliseconds: 220), () => _scan(scroll: false));
  }

  void _clear() {
    final h = _highlight;
    if (h == null) return;
    h.dispose();
    h.p1.dispose();
    h.p2.dispose();
    _highlight = null;
  }

  void _scan({bool scroll = true}) {
    _hits = findTerminalText(widget.tab.terminal, _query);
    _index = _hits.isEmpty ? 0 : _index.clamp(0, _hits.length - 1);
    _paint(scroll: scroll);
    setState(() {});
  }

  void _paint({required bool scroll}) {
    _clear();
    if (_hits.isEmpty) return;
    final match = _hits[_index], buffer = widget.tab.terminal.buffer;
    _highlight = widget.tab.controller.highlight(
      p1: buffer.createAnchorFromOffset(match.start),
      p2: buffer.createAnchorFromOffset(match.end),
      color: GlassTokens.of(context).palette.warning.withValues(alpha: .45),
    );
    if (scroll) widget.reveal(match.start.y);
  }

  void _move(int step) {
    if (_hits.isEmpty) return;
    setState(() => _index = (_index + step) % _hits.length);
    _paint(scroll: true);
    _field.requestFocus();
  }

  @override
  void dispose() {
    _refresh?.cancel();
    _terminal.removeListener(_output);
    _clear();
    _field.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    Widget button(IconData icon, String label, VoidCallback? press) => GlassIconButton(
      icon: icon,
      tooltip: label,
      onPressed: press,
      size: GlassSizes.tab,
      style: GlassIconButtonStyle.plain,
    );
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): widget.close,
        const SingleActivator(LogicalKeyboardKey.enter, shift: true): () => _move(-1),
      },
      child: Row(
        children: [
          Expanded(
            child: TextField(
              key: const ValueKey('terminal-find-input'),
              autofocus: true,
              focusNode: _field,
              maxLines: 1,
              decoration: InputDecoration(isDense: true, hintText: l.terminalFindText),
              onChanged: (v) {
                _query = v;
                _index = 0;
                _scan();
              },
              onSubmitted: (_) => _move(1),
            ),
          ),
          if (_query.isNotEmpty)
            Text(
              _hits.isEmpty ? '0' : '${_index + 1}/${_hits.length}',
              style: GlassTokens.of(context).typography.caption,
            ),
          button(Icons.keyboard_arrow_up, l.automationInventoryPrevious, _hits.isEmpty ? null : () => _move(-1)),
          button(Icons.keyboard_arrow_down, l.commonNext, _hits.isEmpty ? null : () => _move(1)),
          button(Icons.close, l.commonClose, widget.close),
        ],
      ),
    );
  }
}
