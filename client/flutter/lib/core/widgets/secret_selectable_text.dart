import 'dart:async';

import 'package:material_ui/material_ui.dart';

/// Selectable text that shows a revealed secret (S8/S9, ТР-14/15).
///
/// The standard Copy of [SelectableText] writes an ordinary clipboard entry
/// (history, cloud clipboard, no auto-clear). Here both the context menu's
/// Copy and the Ctrl/Cmd+C shortcut call [onCopy] with the selected text, so
/// the caller sends it through `copySecretWithNotice` / `copyPrivate`. Cut,
/// Share, Look Up and web search are not offered.
class SecretSelectableText extends StatefulWidget {
  const SecretSelectableText(this.text, {required this.onCopy, this.style, super.key});

  final String text;
  final TextStyle? style;

  /// Receives the non-empty selected text.
  final Future<void> Function(String selected) onCopy;

  @override
  State<SecretSelectableText> createState() => _SecretSelectableTextState();
}

class _SecretSelectableTextState extends State<SecretSelectableText> {
  TextSelection _selection = const TextSelection.collapsed(offset: -1);

  static const _hidden = {
    ContextMenuButtonType.cut,
    ContextMenuButtonType.share,
    ContextMenuButtonType.lookUp,
    ContextMenuButtonType.searchWeb,
  };

  void _copy(TextSelection selection) {
    if (!selection.isValid || selection.isCollapsed || selection.end > widget.text.length) return;
    final selected = selection.textInside(widget.text);
    if (selected.isNotEmpty) unawaited(widget.onCopy(selected));
  }

  @override
  Widget build(BuildContext context) => Actions(
    // EditableText's copy action is overridable from an ancestor.
    actions: {
      CopySelectionTextIntent: CallbackAction<CopySelectionTextIntent>(
        onInvoke: (_) {
          _copy(_selection);
          return null;
        },
      ),
    },
    child: SelectableText(
      widget.text,
      style: widget.style,
      onSelectionChanged: (selection, _) => _selection = selection,
      contextMenuBuilder: (context, state) => AdaptiveTextSelectionToolbar.buttonItems(
        anchors: state.contextMenuAnchors,
        buttonItems: [
          for (final item in state.contextMenuButtonItems)
            if (item.type == ContextMenuButtonType.copy)
              item.copyWith(
                onPressed: () {
                  final selection = state.textEditingValue.selection;
                  state.hideToolbar();
                  _copy(selection);
                },
              )
            else if (!_hidden.contains(item.type))
              item,
        ],
      ),
    ),
  );
}
