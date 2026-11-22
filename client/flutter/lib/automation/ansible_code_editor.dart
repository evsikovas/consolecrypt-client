import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/automation/ansible_editor_controller.dart';
import 'package:consolecrypt/automation/ansible_editor_folding.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

export 'ansible_editor_controller.dart' show AnsibleEditorViewController;

/// Pass ONLY the buffer that may be displayed: the root's masked controller
/// while hidden/read-only, or raw controller while explicitly revealed.
/// `controller.text` remains complete canonical save text, never a projection.
class AnsibleCodeEditor extends StatefulWidget {
  const AnsibleCodeEditor({
    super.key,
    required this.controller,
    this.readOnly = false,
    this.path,
    this.fieldKey = const ValueKey('automation-editor'),
    this.showLineNumbers = true,
    this.enableFolding = true,
    this.highlightSyntax = true,
    this.viewController,
    this.onChanged,
    this.onCopySelection,
    this.focusNode,
    this.foldTooltip,
    this.limitMessage,
  });
  final TextEditingController controller;
  final bool readOnly, showLineNumbers, enableFolding, highlightSyntax;
  final String? path, limitMessage;
  final Key fieldKey;
  final FocusNode? focusNode;
  final AnsibleEditorViewController? viewController;
  final ValueChanged<String>? onChanged;

  /// Host applies the app clipboard policy to canonical displayed source and
  /// reports success. Missing/failed callbacks never copy or remove selection.
  final Future<bool> Function(String text)? onCopySelection;
  final String Function(bool collapsed, int hiddenLines)? foldTooltip;
  @override
  State<AnsibleCodeEditor> createState() => _AnsibleCodeEditorState();
}

class _AnsibleCodeEditorState extends State<AnsibleCodeEditor> {
  double fontSize = 13;
  late AnsibleEditorDocument document;
  final vertical = ScrollController(), horizontal = ScrollController();
  final ownFocus = FocusNode();
  @override
  void initState() {
    super.initState();
    _create();
  }

  void _create() {
    document = AnsibleEditorDocument(widget.controller, readOnly: widget.readOnly, onChanged: widget.onChanged)
      ..addListener(_changed);
    widget.viewController?.attach(document);
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _destroy() {
    widget.viewController?.attach(null);
    document.removeListener(_changed);
    document.dispose();
  }

  @override
  void didUpdateWidget(covariant AnsibleCodeEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.controller, oldWidget.controller)) {
      oldWidget.viewController?.attach(null);
      document.removeListener(_changed);
      document.dispose();
      _create();
    } else {
      if (oldWidget.viewController != widget.viewController) {
        oldWidget.viewController?.attach(null);
        widget.viewController?.attach(document);
      }
      document
        ..readOnly = widget.readOnly
        ..onChanged = widget.onChanged;
      if (widget.path != oldWidget.path) document.reset();
      if (!widget.enableFolding && oldWidget.enableFolding) document.expandAll();
    }
  }

  @override
  void dispose() {
    _destroy();
    vertical.dispose();
    horizontal.dispose();
    ownFocus.dispose();
    super.dispose();
  }

  Future<void> _copy({bool cut = false}) async {
    final doc = document;
    if (doc.disposed || cut && widget.readOnly) return;
    final selection = doc.selectedSource;
    if (selection.isCollapsed) return;
    final revision = doc.revision;
    final copy = widget.onCopySelection;
    if (copy == null) return;
    bool copied;
    try {
      copied = await copy(doc.selectedText);
    } catch (_) {
      // The host owns copy failure feedback. A failed write must not cut data.
      return;
    }
    if (copied &&
        cut &&
        mounted &&
        identical(doc, document) &&
        !doc.disposed &&
        revision == doc.revision &&
        selection == doc.selectedSource &&
        !widget.readOnly) {
      doc.replaceSelection('');
    }
  }

  Future<void> _paste() async {
    if (widget.readOnly) return;
    final doc = document, revision = document.revision, selection = document.selectedSource;
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    if (!mounted ||
        !identical(doc, document) ||
        doc.disposed ||
        widget.readOnly ||
        doc.revision != revision ||
        selection != doc.selectedSource) {
      return;
    }
    if (data?.text case final String text) doc.replaceSelection(text);
  }

  Widget _toolbar(BuildContext context, EditableTextState state) {
    final items = <ContextMenuButtonItem>[];
    for (final item in state.contextMenuButtonItems) {
      final VoidCallback? callback = switch (item.type) {
        ContextMenuButtonType.copy when widget.onCopySelection != null => () => unawaited(_copy()),
        ContextMenuButtonType.cut when !widget.readOnly && widget.onCopySelection != null => () => unawaited(
          _copy(cut: true),
        ),
        ContextMenuButtonType.paste when !widget.readOnly => () => unawaited(_paste()),
        ContextMenuButtonType.selectAll => document.selectAll,
        ContextMenuButtonType.delete when !widget.readOnly => () => document.replaceSelection(''),
        _ => null,
      };
      // Share/search/look-up must not export the synthetic projection or bypass
      // canonical/masked copy policy through a platform-specific action.
      if (callback != null) {
        items.add(
          ContextMenuButtonItem(
            type: item.type,
            label: item.label,
            onPressed: () {
              state.hideToolbar();
              callback();
            },
          ),
        );
      }
    }
    return AdaptiveTextSelectionToolbar.buttonItems(anchors: state.contextMenuAnchors, buttonItems: items);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final style = tokens.typography.mono.copyWith(fontSize: fontSize, height: 1.5);
    final lineHeight = MediaQuery.textScalerOf(context).scale(fontSize) * 1.5;
    document.display.highlight = widget.highlightSyntax;
    final double contentWidth;
    if (document.display.text.length > 200000) {
      // Avoid a second paragraph layout for the large-file plain-text path.
      // Conservative width prevents wrapping; EditableText owns actual layout.
      final longest = ansibleSourceLines(document.display.text)
          .fold<int>(0, (max, line) => math.max(max, line.text.length));
      contentWidth = longest * MediaQuery.textScalerOf(context).scale(fontSize) * 1.2 + 32;
    } else {
      final metrics = TextPainter(
        text: TextSpan(text: document.display.text, style: style),
        textDirection: TextDirection.ltr,
        textScaler: MediaQuery.textScalerOf(context),
      )..layout();
      contentWidth = metrics.width + 32;
      metrics.dispose();
    }
    final digits = math.max(2, '${document.projection.sourceLines.length}'.length);
    final gutterWidth =
        (widget.showLineNumbers ? digits * MediaQuery.textScalerOf(context).scale(fontSize) * .75 + 12 : 0) +
        (widget.enableFolding ? 24 : 0);
    final actions = <Type, Action<Intent>>{
      _EditorZoomIntent: CallbackAction<_EditorZoomIntent>(
        onInvoke: (intent) {
          final next = (fontSize + intent.delta).clamp(8.0, 32.0);
          if (next != fontSize) setState(() => fontSize = next);
          return null;
        },
      ),
      CopySelectionTextIntent: CallbackAction<CopySelectionTextIntent>(
        onInvoke: (intent) {
          unawaited(_copy(cut: intent.collapseSelection));
          return null;
        },
      ),
      PasteTextIntent: CallbackAction<PasteTextIntent>(
        onInvoke: (_) {
          unawaited(_paste());
          return null;
        },
      ),
      SelectAllTextIntent: CallbackAction<SelectAllTextIntent>(
        onInvoke: (_) {
          document.selectAll();
          return null;
        },
      ),
      UndoTextIntent: CallbackAction<UndoTextIntent>(
        onInvoke: (_) {
          document.undo();
          return null;
        },
      ),
      RedoTextIntent: CallbackAction<RedoTextIntent>(
        onInvoke: (_) {
          document.redo();
          return null;
        },
      ),
    };
    return Container(
      key: const ValueKey('ansible-editor-frame'),
      clipBehavior: Clip.hardEdge,
      decoration: BoxDecoration(color: tokens.surfaces.paper, borderRadius: BorderRadius.circular(8)),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (document.editRejected)
            Text(
              widget.limitMessage ?? context.l10n.errorPayloadTooLarge,
              style: TextStyle(color: tokens.palette.danger),
            ),
          Expanded(
            child: LayoutBuilder(
              builder: (context, bounds) => Scrollbar(
                controller: vertical,
                child: SingleChildScrollView(
                  controller: vertical,
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      if (gutterWidth > 0)
                        SizedBox(
                          width: gutterWidth.toDouble(),
                          child: Padding(
                            padding: const EdgeInsets.only(top: 8, bottom: 8),
                            child: Column(
                              children: [
                                for (var i = 0; i < document.projection.lineNumbers.length; i++)
                                  _gutter(context, i, lineHeight, style),
                              ],
                            ),
                          ),
                        ),
                      Expanded(
                        child: SingleChildScrollView(
                          controller: horizontal,
                          scrollDirection: Axis.horizontal,
                          child: SizedBox(
                            width: math.max(math.max(80, bounds.maxWidth - gutterWidth), contentWidth),
                            child: Shortcuts(
                              shortcuts: const {
                                SingleActivator(LogicalKeyboardKey.equal, control: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.equal, control: true, shift: true):
                                    _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.add, control: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.add, control: true, shift: true): _EditorZoomIntent(
                                  1,
                                ),
                                SingleActivator(LogicalKeyboardKey.numpadAdd, control: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.minus, control: true): _EditorZoomIntent(-1),
                                SingleActivator(LogicalKeyboardKey.numpadSubtract, control: true): _EditorZoomIntent(
                                  -1,
                                ),
                                SingleActivator(LogicalKeyboardKey.equal, meta: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.equal, meta: true, shift: true): _EditorZoomIntent(
                                  1,
                                ),
                                SingleActivator(LogicalKeyboardKey.add, meta: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.add, meta: true, shift: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.numpadAdd, meta: true): _EditorZoomIntent(1),
                                SingleActivator(LogicalKeyboardKey.minus, meta: true): _EditorZoomIntent(-1),
                                SingleActivator(LogicalKeyboardKey.numpadSubtract, meta: true): _EditorZoomIntent(-1),
                              },
                              child: Actions(
                                actions: actions,
                                child: TextField(
                                  key: widget.fieldKey,
                                  controller: document.display,
                                  focusNode: widget.focusNode ?? ownFocus,
                                  readOnly: widget.readOnly,
                                  maxLines: null,
                                  minLines: 1,
                                  style: style,
                                  strutStyle: StrutStyle.fromTextStyle(style, forceStrutHeight: true),
                                  textAlignVertical: TextAlignVertical.top,
                                  textDirection: TextDirection.ltr,
                                  autocorrect: false,
                                  enableSuggestions: false,
                                  enableIMEPersonalizedLearning: false,
                                  smartDashesType: SmartDashesType.disabled,
                                  smartQuotesType: SmartQuotesType.disabled,
                                  contextMenuBuilder: _toolbar,
                                  decoration: const InputDecoration(
                                    filled: false,
                                    border: InputBorder.none,
                                    enabledBorder: InputBorder.none,
                                    focusedBorder: InputBorder.none,
                                    disabledBorder: InputBorder.none,
                                    isDense: true,
                                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _gutter(BuildContext context, int index, double height, TextStyle style) {
    final number = document.projection.lineNumbers[index];
    final region = number == null ? null : document.regions.where((region) => region.headerLine == number).firstOrNull;
    final folded = region != null && document.collapsed.contains(region.headerOffset);
    final local = MaterialLocalizations.of(context);
    return SizedBox(
      height: height,
      child: Row(
        mainAxisAlignment: MainAxisAlignment.end,
        children: [
          if (widget.showLineNumbers)
            Expanded(
              child: ExcludeSemantics(
                child: Text(
                  number?.toString() ?? '…',
                  key: ValueKey('ansible-line-$index'),
                  textAlign: TextAlign.right,
                  strutStyle: StrutStyle.fromTextStyle(style, forceStrutHeight: true),
                  style: style.copyWith(color: GlassTokens.of(context).secondaryLabel),
                ),
              ),
            ),
          if (widget.enableFolding)
            SizedBox(
              width: 24,
              height: height,
              child: region == null
                  ? null
                  : Tooltip(
                      message:
                          widget.foldTooltip?.call(folded, region.hiddenLines) ??
                          '${folded ? local.collapsedIconTapHint : local.expandedIconTapHint} (${region.hiddenLines})',
                      child: IconButton(
                        key: ValueKey('ansible-fold-$number'),
                        padding: EdgeInsets.zero,
                        constraints: const BoxConstraints(),
                        iconSize: 16,
                        onPressed: () => document.toggleFold(number!),
                        icon: Icon(folded ? Icons.chevron_right : Icons.expand_more),
                      ),
                    ),
            ),
        ],
      ),
    );
  }
}

class _EditorZoomIntent extends Intent {
  const _EditorZoomIntent(this.delta);
  final double delta;
}
