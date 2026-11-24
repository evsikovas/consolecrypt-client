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

class TerminalStatusBar extends StatelessWidget {
  const TerminalStatusBar({required this.history, super.key});
  final ValueListenable<HostMetricsHistory> history;

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
      Widget metric(String label, IconData icon, String value, Color color) => Tooltip(
        message: label,
        child: Semantics(
          label: '$label $value',
          excludeSemantics: true,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 13, color: color),
              const SizedBox(width: 4),
              Text(value, style: t.typography.mono.copyWith(fontSize: 11, color: color)),
            ],
          ),
        ),
      );
      return Container(
        key: const ValueKey('terminal-status-bar'),
        decoration: BoxDecoration(
          color: t.surfaces.inset,
          border: Border(top: BorderSide(color: t.surfaces.separator)),
        ),
        padding: const EdgeInsets.symmetric(vertical: 4, horizontal: 6),
        child: Row(
          children: [
            Expanded(
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  spacing: 12,
                  children: [
                    metric(l.hostMetricsCpu, Icons.memory, percent(s.cpu), t.palette.success),
                    metric(
                      l.hostMetricsRam,
                      Icons.view_stream_outlined,
                      memory(s.memoryUsed, s.memoryTotal),
                      t.palette.warning,
                    ),
                    if ((s.swapTotal ?? 0) > 0)
                      metric(l.sessionSwap, Icons.swap_horiz, memory(s.swapUsed, s.swapTotal), t.palette.secondary),
                    metric(l.hostMetricsDisk, Icons.storage, percent(s.diskPercent), t.palette.success),
                    metric(l.hostMetricsSent, Icons.arrow_upward, rate(s.sent), t.palette.info),
                    metric(l.hostMetricsReceived, Icons.arrow_downward, rate(s.received), t.palette.info),
                    if (load != null && (s.logicalCpus ?? 0) > 0)
                      metric(
                        l.terminalNormalizedLoad,
                        Icons.speed,
                        percent(load * 100 / s.logicalCpus!),
                        t.palette.success,
                      ),
                  ],
                ),
              ),
            ),
            if (s.channelLatencyMs case final ms?)
              Padding(
                padding: const EdgeInsets.only(left: 6),
                child: metric(
                  l.terminalChannelLatency,
                  Icons.monitor_heart_outlined,
                  l.terminalLatencyValue(ms),
                  t.palette.success,
                ),
              ),
          ],
        ),
      );
    },
  );
}
