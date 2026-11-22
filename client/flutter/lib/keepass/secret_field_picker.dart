import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// The picker receives labels and callbacks only. A field's value is never rendered.
class SecretFieldPicker extends StatefulWidget {
  const SecretFieldPicker({
    super.key,
    required this.path,
    required this.engine,
    required this.fields,
    required this.onBack,
  });
  final String path, engine;
  final List<({Key? key, String label, VoidCallback? copy})> fields;
  final VoidCallback onBack;
  @override
  State<SecretFieldPicker> createState() => _PickerState();
}

class _PickerState extends State<SecretFieldPicker> {
  final focus = FocusNode();
  final scroll = ScrollController();
  int index = 0;
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) focus.requestFocus();
    });
  }

  @override
  void dispose() {
    focus.dispose();
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final fields = widget.fields;
    int current() => index.clamp(0, fields.isEmpty ? 0 : fields.length - 1);
    final height = 44.0 + (MediaQuery.textScalerOf(context).scale(16) - 16).clamp(0, 80);
    void move(int delta) {
      if (fields.isEmpty) return;
      final next = (current() + delta).clamp(0, fields.length - 1);
      setState(() => index = next);
      if (scroll.hasClients) {
        final start = next * height, end = start + height;
        if (start < scroll.offset) scroll.jumpTo(start);
        if (end > scroll.offset + scroll.position.viewportDimension) {
          scroll.jumpTo((end - scroll.position.viewportDimension).clamp(0, scroll.position.maxScrollExtent));
        }
      }
    }

    return Focus(
      focusNode: focus,
      onKeyEvent: (_, e) {
        if (e is! KeyDownEvent && e is! KeyRepeatEvent) return KeyEventResult.ignored;
        if (e.logicalKey == LogicalKeyboardKey.escape) {
          widget.onBack();
        } else if (e.logicalKey == LogicalKeyboardKey.arrowDown) {
          move(1);
        } else if (e.logicalKey == LogicalKeyboardKey.arrowUp) {
          move(-1);
        } else if (e.logicalKey == LogicalKeyboardKey.enter || e.logicalKey == LogicalKeyboardKey.numpadEnter) {
          if (fields.isNotEmpty && e is KeyDownEvent) fields[current()].copy?.call();
        } else {
          return KeyEventResult.ignored;
        }
        return KeyEventResult.handled;
      },
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
            child: Row(
              children: [
                GlassIconButton(
                  icon: Icons.arrow_back,
                  tooltip: l.commonBack,
                  onPressed: widget.onBack,
                  style: GlassIconButtonStyle.plain,
                ),
                Expanded(
                  child: Tooltip(
                    message: widget.path,
                    child: Text(
                      widget.path,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.mono,
                    ),
                  ),
                ),
                Text(widget.engine, style: tokens.typography.caption),
              ],
            ),
          ),
          const Divider(height: 1),
          Flexible(
            child: ListView.builder(
              shrinkWrap: true,
              primary: false,
              controller: scroll,
              itemExtent: height,
              itemCount: fields.length,
              itemBuilder: (context, i) {
                final field = fields[i], selected = i == current();
                return Semantics(
                  selected: selected,
                  button: true,
                  label: field.label,
                  child: MouseRegion(
                    onEnter: (_) => setState(() => index = i),
                    child: Material(
                      color: selected ? tokens.sidebarSelection : Colors.transparent,
                      child: InkWell(
                        key: field.key,
                        onTap: field.copy,
                        child: Container(
                          padding: const EdgeInsets.symmetric(horizontal: 12),
                          decoration: BoxDecoration(
                            border: Border(bottom: BorderSide(color: tokens.surfaces.separator)),
                          ),
                          child: Row(
                            spacing: 8,
                            children: [
                              const Icon(Icons.key_outlined, size: 16),
                              Expanded(child: Text(field.label, maxLines: 1, overflow: TextOverflow.ellipsis)),
                              ExcludeSemantics(
                                child: Text(
                                  '••••••••',
                                  style: tokens.typography.mono.copyWith(color: tokens.secondaryLabel),
                                ),
                              ),
                              if (selected)
                                GlassButton(icon: Icons.copy_outlined, label: l.commonCopy, onPressed: field.copy)
                              else
                                GlassIconButton(
                                  icon: Icons.copy_outlined,
                                  tooltip: l.commonCopy,
                                  onPressed: field.copy,
                                  style: GlassIconButtonStyle.plain,
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(8),
            child: Wrap(
              alignment: WrapAlignment.spaceBetween,
              spacing: 12,
              runSpacing: 4,
              children: [
                Text('↑ ↓   ${l.uiFieldNavigation}', style: tokens.typography.caption),
                Text('Enter   ${l.commonCopy}', style: tokens.typography.caption), // l10n-ignore: keyboard keycap with localized action.
              ],
            ),
          ),
        ],
      ),
    );
  }
}
