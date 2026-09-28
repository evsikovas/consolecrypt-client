import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:material_ui/material_ui.dart';

/// A single selectable document with a separate, nonselectable number gutter.
/// Long lines scroll horizontally instead of changing file line numbers.
class NumberedTextPreview extends StatefulWidget {
  const NumberedTextPreview({required this.text, super.key});

  final String text;

  @override
  State<NumberedTextPreview> createState() => _NumberedTextPreviewState();
}

class _NumberedTextPreviewState extends State<NumberedTextPreview> {
  final _vertical = ScrollController();
  final _horizontal = ScrollController();

  @override
  void dispose() {
    _vertical.dispose();
    _horizontal.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final scaler = MediaQuery.textScalerOf(context);
    final style = DefaultTextStyle.of(context).style.copyWith(
      fontFamily: AppPlatform.monospaceFamily,
      fontFamilyFallback: AppPlatform.monospaceFallback,
      fontSize: 12.5,
      height: 1.4,
      letterSpacing: 0,
      wordSpacing: 0,
      color: colors.onSurface,
    );
    final strut = StrutStyle.fromTextStyle(style, forceStrutHeight: true);
    final measure = TextPainter(
      text: TextSpan(text: widget.text, style: style),
      textDirection: TextDirection.ltr,
      textScaler: scaler,
      strutStyle: strut,
    )..layout();
    final documentWidth = measure.width + 4; // caret margin, no soft wraps
    final lines = math.max(1, measure.computeLineMetrics().length);
    measure.text = TextSpan(text: '$lines', style: style);
    measure.layout();
    final gutterWidth = measure.width + 24;
    measure.dispose();
    return ColoredBox(
      color: colors.surfaceContainerLow,
      child: Scrollbar(
        controller: _horizontal,
        thumbVisibility: true,
        scrollbarOrientation: ScrollbarOrientation.bottom,
        notificationPredicate: (notification) => notification.metrics.axis == Axis.horizontal,
        child: Scrollbar(
          controller: _vertical,
          notificationPredicate: (notification) =>
              notification.depth == 0 && notification.metrics.axis == Axis.vertical,
          thumbVisibility: true,
          child: SingleChildScrollView(
            key: const ValueKey('sftp-preview-vertical'),
            controller: _vertical,
            padding: const EdgeInsets.symmetric(vertical: 12),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              textDirection: TextDirection.ltr,
              children: [
                ExcludeSemantics(
                  child: SelectionContainer.disabled(
                    child: Container(
                      width: gutterWidth,
                      padding: const EdgeInsets.symmetric(horizontal: 10),
                      decoration: BoxDecoration(
                        border: Border(right: BorderSide(color: colors.outlineVariant)),
                      ),
                      child: Text(
                        List.generate(lines, (i) => '${i + 1}').join('\n'),
                        key: const ValueKey('sftp-preview-line-numbers'),
                        textDirection: TextDirection.ltr,
                        textAlign: TextAlign.right,
                        style: style.copyWith(color: colors.onSurfaceVariant),
                        strutStyle: strut,
                      ),
                    ),
                  ),
                ),
                Expanded(
                  child: LayoutBuilder(
                    builder: (context, constraints) => SingleChildScrollView(
                      key: const ValueKey('sftp-preview-horizontal'),
                      controller: _horizontal,
                      scrollDirection: Axis.horizontal,
                      padding: const EdgeInsets.fromLTRB(12, 0, 16, 12),
                      child: SizedBox(
                        width: math.max(documentWidth, constraints.maxWidth - 28),
                        child: SelectableText(
                          widget.text,
                          key: const ValueKey('sftp-quicklook-text'),
                          textDirection: TextDirection.ltr,
                          style: style,
                          strutStyle: strut,
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
    );
  }
}
