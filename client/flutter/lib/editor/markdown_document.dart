import 'dart:async';

import 'package:consolecrypt/core/glass/glass_segmented.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:material_ui/material_ui.dart';
import 'package:url_launcher/url_launcher.dart';

final markdownBrowserProvider = Provider<Future<bool> Function(Uri)>(
  (ref) =>
      (uri) => launchUrl(uri, mode: LaunchMode.externalApplication, webOnlyWindowName: '_blank'),
);

/// Preview and source use the same buffer, including edits still on the device.
class MarkdownDocument extends ConsumerStatefulWidget {
  const MarkdownDocument({
    required this.controller,
    required this.source,
    required this.codeStyle,
    required this.onCopy,
    this.onOpenFileLink,
    super.key,
  });

  final TextEditingController controller;
  final Widget source;
  final TextStyle codeStyle;
  final ValueChanged<String>? onOpenFileLink;
  final Future<bool> Function(String) onCopy;

  @override
  ConsumerState<MarkdownDocument> createState() => _MarkdownDocumentState();
}

class _MarkdownDocumentState extends ConsumerState<MarkdownDocument> {
  bool preview = true;
  bool openedSource = false;

  Future<void> _openLink(String? href) async {
    if (href == null || href.isEmpty) return;
    var uri = Uri.tryParse(href);
    if (uri == null) return;
    if (!uri.hasScheme && !uri.hasAuthority) {
      widget.onOpenFileLink?.call(href);
      return;
    }
    if (!uri.hasScheme) uri = uri.replace(scheme: 'https');
    if ((uri.scheme != 'http' && uri.scheme != 'https') || uri.host.isEmpty) return;
    try {
      if (await ref.read(markdownBrowserProvider)(uri)) return;
    } catch (_) {
      // The platform may have no browser registered.
    }
    if (mounted) showSnack(context, context.l10n.editorBrowserFailed, error: true);
  }

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
                    onTapLink: (_, href, _) => _openLink(href),
                    builders: {
                      'pre': _CodeBlockBuilder(widget.codeStyle.copyWith(color: colors.onSurface), widget.onCopy),
                    },
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

class _CodeBlockBuilder extends MarkdownElementBuilder {
  _CodeBlockBuilder(this.style, this.copy);
  final TextStyle style;
  final Future<bool> Function(String) copy;

  @override
  Widget visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    // Markdown terminates each block with a newline, including single-line commands.
    final raw = element.textContent;
    final code = raw.endsWith('\n') ? raw.substring(0, raw.length - 1) : raw;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Text(code, style: style),
          ),
        ),
        _CopyCodeButton(key: ValueKey(code), code: code, copy: copy),
      ],
    );
  }
}

class _CopyCodeButton extends StatefulWidget {
  const _CopyCodeButton({required super.key, required this.code, required this.copy});
  final String code;
  final Future<bool> Function(String) copy;

  @override
  State<_CopyCodeButton> createState() => _CopyCodeButtonState();
}

class _CopyCodeButtonState extends State<_CopyCodeButton> {
  Timer? reset;
  bool copied = false;

  Future<void> _copy() async {
    if (!await widget.copy(widget.code) || !mounted) return;
    reset?.cancel();
    setState(() => copied = true);
    reset = Timer(const Duration(seconds: 2), () => setState(() => copied = false));
  }

  @override
  void dispose() {
    reset?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: context.l10n.commonCopy,
    onPressed: _copy,
    iconSize: 16,
    icon: Icon(copied ? Icons.check : Icons.copy_outlined),
  );
}
