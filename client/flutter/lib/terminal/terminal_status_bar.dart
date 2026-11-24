import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/terminal/terminal_encoding.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

class TerminalEncodingButton extends ConsumerWidget {
  const TerminalEncodingButton({required this.tab, super.key});
  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    String group(TerminalEncoding e) => switch (e) {
      TerminalEncoding.utf8 => '',
      TerminalEncoding.gb2312 ||
      TerminalEncoding.gbk ||
      TerminalEncoding.gb18030 => l.terminalEncodingSimplifiedChinese,
      TerminalEncoding.big5 => l.terminalEncodingTraditionalChinese,
      TerminalEncoding.shiftJis || TerminalEncoding.eucJp => l.terminalEncodingJapanese,
      TerminalEncoding.eucKr => l.terminalEncodingKorean,
      TerminalEncoding.windows1252 ||
      TerminalEncoding.latin1 ||
      TerminalEncoding.latin9 => l.terminalEncodingWesternEuropean,
      TerminalEncoding.windows1250 || TerminalEncoding.latin2 => l.terminalEncodingCentralEuropean,
      TerminalEncoding.windows1251 || TerminalEncoding.koi8r => l.terminalEncodingCyrillic,
    };
    return GlassMenuButton<TerminalEncoding>(
      entries: [
        for (final e in TerminalEncoding.values) ...[
          if (const {
            TerminalEncoding.gb2312,
            TerminalEncoding.big5,
            TerminalEncoding.shiftJis,
            TerminalEncoding.eucKr,
            TerminalEncoding.windows1252,
            TerminalEncoding.windows1251,
          }.contains(e))
            const GlassMenuDivider(),
          GlassMenuItem(
            value: e,
            checked: tab.encoding == e,
            label: [e.label, if (group(e).isNotEmpty) group(e)].join(' · '),
          ),
        ],
      ],
      onSelected: (e) => ref.read(terminalTabsProvider.notifier).setEncoding(tab, e),
      builder: (_, open) => GlassIconButton(
        key: const ValueKey('terminal-encoding'),
        icon: Icons.translate_rounded,
        tooltip: '${l.dbCsvEncoding} · ${tab.encoding.label}',
        size: GlassSizes.tab,
        iconSize: GlassSizes.iconRow,
        style: GlassIconButtonStyle.plain,
        onPressed: open,
      ),
    );
  }
}

typedef _Metric = ({String label, IconData icon, String value});

class TerminalStatusBar extends StatelessWidget {
  const TerminalStatusBar({required this.history, this.foreground, super.key});
  final ValueListenable<HostMetricsHistory> history;
  final Color? foreground;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<HostMetricsHistory>(
    valueListenable: history,
    builder: (context, history, _) {
      final s = history.latest;
      if (s == null || history.error != null) return const SizedBox.shrink();
      final t = GlassTokens.of(context), l = context.l10n;
      final pct = NumberFormat.decimalPercentPattern(locale: l.localeName, decimalDigits: 0);
      String percent(double? value) => value == null ? '—' : pct.format(value / 100);
      String memory(int? used, int? total) =>
          used == null || total == null ? '—' : '${formatBytes(l, used)} / ${formatBytes(l, total)}';
      String rate(double? value) => value == null ? '—' : l.sftpTransferSpeed(formatBytes(l, value.round()));
      final load = double.tryParse(s.loadAverage?.split('/').first.trim() ?? '');
      final metrics = <_Metric>[
        (label: l.hostMetricsCpu, icon: Icons.memory, value: percent(s.cpu)),
        (label: l.hostMetricsRam, icon: Icons.view_stream_outlined, value: memory(s.memoryUsed, s.memoryTotal)),
        (label: l.hostMetricsDisk, icon: Icons.storage, value: percent(s.diskPercent)),
        (label: l.hostMetricsReceived, icon: Icons.arrow_downward, value: rate(s.received)),
        (label: l.hostMetricsSent, icon: Icons.arrow_upward, value: rate(s.sent)),
        if ((s.swapTotal ?? 0) > 0)
          (label: l.sessionSwap, icon: Icons.swap_horiz, value: memory(s.swapUsed, s.swapTotal)),
        if (load != null && (s.logicalCpus ?? 0) > 0)
          (label: l.terminalNormalizedLoad, icon: Icons.speed, value: percent(load * 100 / s.logicalCpus!)),
      ];
      final ms = s.channelLatencyMs;
      final _Metric? latency = ms == null
          ? null
          : (label: l.terminalChannelLatency, icon: Icons.monitor_heart_outlined, value: l.terminalLatencyValue(ms));
      final ink = foreground ?? t.palette.label;
      final style = DefaultTextStyle.of(context).style
          .merge(t.typography.mono)
          .copyWith(color: ink.withValues(alpha: t.highContrast ? 1 : .76), fontSize: 11, height: 1.2);
      final iconColor = ink.withValues(alpha: t.highContrast ? 1 : .60);
      const iconSize = 12.0, iconGap = 4.0, gap = 12.0, moreWidth = 24.0;
      double width(_Metric m) =>
          iconSize +
          iconGap +
          TextPainter.computeWidth(
            text: TextSpan(text: m.value, style: style),
            textDirection: Directionality.of(context),
            textScaler: MediaQuery.textScalerOf(context),
            locale: Localizations.maybeLocaleOf(context),
            maxLines: 1,
          ).ceilToDouble();
      Widget inline(_Metric m) => Tooltip(
        message: m.label,
        child: Semantics(
          label: '${m.label} ${m.value}',
          excludeSemantics: true,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(m.icon, size: iconSize, color: iconColor),
              const SizedBox(width: iconGap),
              Text(m.value, style: style, maxLines: 1, softWrap: false),
            ],
          ),
        ),
      );
      return DecoratedBox(
        key: const ValueKey('terminal-status-bar'),
        decoration: BoxDecoration(
          border: Border(top: BorderSide(color: ink.withValues(alpha: .12), width: 0)),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 4),
          child: LayoutBuilder(
            builder: (context, constraints) {
              final widths = metrics.map(width).toList();
              final latencyWidth = latency == null ? 0.0 : width(latency);
              final total =
                  widths.fold(0.0, (a, b) => a + b) + latencyWidth + gap * (metrics.length - (latency == null ? 1 : 0));
              final overflow = total > constraints.maxWidth;
              final pinLatency =
                  latency != null && (!overflow || latencyWidth + gap + moreWidth <= constraints.maxWidth);
              var remaining =
                  constraints.maxWidth - (overflow ? moreWidth + gap : 0) - (pinLatency ? latencyWidth + gap : 0);
              var shown = 0;
              while (shown < widths.length && (!overflow || widths[shown] <= remaining)) {
                remaining -= widths[shown++] + gap;
              }
              final hidden = [...metrics.skip(shown), if (latency != null && !pinLatency) latency];
              return ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 24),
                child: Row(
                  children: [
                    for (var i = 0; i < shown; i++) ...[if (i > 0) const SizedBox(width: gap), inline(metrics[i])],
                    const Spacer(),
                    if (pinLatency) inline(latency),
                    if (hidden.isNotEmpty) ...[
                      if (pinLatency) const SizedBox(width: gap),
                      SizedBox(
                        width: constraints.maxWidth.clamp(0.0, moreWidth),
                        height: 24,
                        child: _MoreMetrics(metrics: hidden, ink: iconColor),
                      ),
                    ],
                  ],
                ),
              );
            },
          ),
        ),
      );
    },
  );
}

class _MoreMetrics extends StatefulWidget {
  const _MoreMetrics({required this.metrics, required this.ink});
  final List<_Metric> metrics;
  final Color ink;
  @override
  State<_MoreMetrics> createState() => _MoreMetricsState();
}

class _MoreMetricsState extends State<_MoreMetrics> {
  final _tooltip = GlobalKey<RawTooltipState>();
  @override
  Widget build(BuildContext context) {
    final t = GlassTokens.of(context);
    final label = context.l10n.terminalMoreMetrics;
    return RawTooltip(
      key: _tooltip,
      semanticsTooltip: label,
      hoverDelay: const Duration(milliseconds: 120),
      dismissDelay: const Duration(milliseconds: 220),
      triggerMode: TooltipTriggerMode.manual,
      positionDelegate: (p) => positionDependentBox(
        size: p.overlaySize,
        childSize: p.tooltipSize,
        target: p.target,
        preferBelow: false,
        verticalOffset: p.targetSize.height / 2,
      ),
      tooltipBuilder: (context, animation) {
        final trigger = _tooltip.currentContext!.findRenderObject()! as RenderBox;
        final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
        final above = trigger.localToGlobal(Offset.zero, ancestor: overlay).dy;
        // Leave room for the overlay margin. On a terminal docked against the
        // top edge, the tooltip positioner can use the space below instead.
        final room = above >= 64 ? above : overlay.size.height - above - trigger.size.height;
        return FadeTransition(
          opacity: animation,
          child: Material(
            key: const ValueKey('terminal-hidden-metrics'),
            color: t.surfaces.contentSolid,
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(8),
              side: BorderSide(color: t.surfaces.separator),
            ),
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: 296, maxHeight: (room - 12).clamp(0, 320)),
              child: SingleChildScrollView(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final m in widget.metrics)
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 4),
                        child: Row(
                          children: [
                            Icon(m.icon, size: 14, color: t.secondaryLabel),
                            const SizedBox(width: 8),
                            Expanded(
                              child: Text(m.label, style: t.typography.callout.copyWith(color: t.secondaryLabel)),
                            ),
                            const SizedBox(width: 8),
                            Flexible(
                              child: Align(
                                alignment: AlignmentDirectional.centerEnd,
                                child: Text(
                                  m.value,
                                  textAlign: TextAlign.end,
                                  style: t.typography.mono.copyWith(fontSize: 11, color: t.palette.label),
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
      child: IconButton(
        key: const ValueKey('terminal-metrics-more'),
        padding: EdgeInsets.zero,
        constraints: const BoxConstraints.tightFor(height: 24),
        iconSize: 16,
        visualDensity: VisualDensity.compact,
        onPressed: () => _tooltip.currentState?.ensureTooltipVisible(),
        icon: Icon(Icons.keyboard_arrow_right_rounded, color: widget.ink, semanticLabel: label),
      ),
    );
  }
}
