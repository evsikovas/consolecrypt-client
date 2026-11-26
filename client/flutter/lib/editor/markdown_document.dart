import 'package:consolecrypt/core/glass/glass_segmented.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:material_ui/material_ui.dart';

/// Preview and source use the same buffer, including edits still on the device.
class MarkdownDocument extends StatefulWidget {
  const MarkdownDocument({required this.controller, required this.source, required this.codeStyle, super.key});

  final TextEditingController controller;
  final Widget source;
  final TextStyle codeStyle;

  @override
  State<MarkdownDocument> createState() => _MarkdownDocumentState();
}

class _MarkdownDocumentState extends State<MarkdownDocument> {
  bool preview = true;
  bool openedSource = false;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final body = theme.textTheme.bodyMedium!.copyWith(fontSize: widget.codeStyle.fontSize, height: 1.5);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
          child: Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassSegmented<bool>(
              selected: preview,
              inChrome: false,
              semanticLabel: 'Markdown',
              segments: [
                GlassSegment(value: true, label: l.notesPreview),
                GlassSegment(value: false, label: l.notesEdit),
              ],
              onChanged: (value) => setState(() {
                preview = value;
                if (!value) openedSource = true;
              }),
            ),
          ),
        ),
        Expanded(
          child: Stack(
            fit: StackFit.expand,
            children: [
              if (openedSource)
                Offstage(
                  offstage: preview,
                  child: ExcludeFocus(excluding: preview, child: widget.source),
                ),
              if (preview)
                ValueListenableBuilder<TextEditingValue>(
                  valueListenable: widget.controller,
                  builder: (context, value, _) => Markdown(
                    data: value.text,
                    padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
                    // A document can come from SSH. Opening it must not read local files or contact image hosts.
                    imageBuilder: (uri, title, alt) => Text(alt?.isNotEmpty == true ? alt! : title ?? uri.toString()),
                    styleSheet: MarkdownStyleSheet(
                      p: body,
                      h1: theme.textTheme.headlineMedium,
                      h2: theme.textTheme.headlineSmall,
                      h3: theme.textTheme.titleLarge,
                      h4: theme.textTheme.titleMedium,
                      h5: theme.textTheme.titleSmall,
                      h6: theme.textTheme.labelLarge,
                      a: body.copyWith(color: colors.primary),
                      listBullet: body,
                      blockquote: body,
                      tableHead: body.copyWith(fontWeight: FontWeight.w600),
                      tableBody: body,
                      tableColumnWidth: const IntrinsicColumnWidth(),
                      tableBorder: TableBorder.all(color: theme.dividerColor),
                      tableCellsPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                      code: widget.codeStyle.copyWith(color: colors.onSurface),
                      codeblockDecoration: BoxDecoration(
                        color: colors.surfaceContainerHighest,
                        borderRadius: BorderRadius.circular(6),
                      ),
                      blockquoteDecoration: BoxDecoration(
                        border: BorderDirectional(start: BorderSide(color: colors.outline, width: 3)),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}
