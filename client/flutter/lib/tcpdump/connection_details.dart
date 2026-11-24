import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:material_ui/material_ui.dart';

String packetTime(CapturePacket packet) => packet.time.toLocal().toIso8601String().split('T').last.substring(0, 12);

class CaptureConnectionDetails extends StatelessWidget {
  const CaptureConnectionDetails({
    required this.packet,
    required this.packets,
    required this.select,
    required this.back,
    super.key,
  });
  final CapturePacket packet;
  final List<CapturePacket> packets;
  final ValueChanged<CapturePacket> select;
  final VoidCallback back;

  @override
  Widget build(BuildContext context) {
    final t = GlassTokens.of(context);
    final l = context.l10n;
    final mono = t.typography.mono.copyWith(fontSize: 12);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            GlassIconButton(icon: Icons.arrow_back_rounded, tooltip: l.commonBack, onPressed: back, size: 28),
            const SizedBox(width: 8),
            Expanded(child: Text(l.captureExchange, style: t.typography.bodyEmph)),
          ],
        ),
        Expanded(
          child: ListView(
            key: const PageStorageKey('tcpdump-exchange'),
            children: [
              GlassPanel(
                padding: const EdgeInsets.all(12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SelectableText('${packet.source} → ${packet.destination}', style: t.typography.bodyEmph),
                    const SizedBox(height: 6),
                    SelectableText(
                      [
                        packet.time.toLocal().toIso8601String().replaceFirst('T', ' '),
                        packet.network,
                        packet.protocol,
                        if (packet.interface case final device?) [device, ?packet.direction].join(' '),
                      ].where((s) => s.isNotEmpty).join(' · '),
                      style: mono,
                    ),
                    if (packet.summary.isNotEmpty) SelectableText(packet.summary, style: mono),
                    if (packet.sequence != null || packet.ack != null || packet.window != null)
                      SelectableText(
                        [
                          if (packet.sequence != null) 'seq ${packet.sequence}',
                          if (packet.ack != null) 'ack ${packet.ack}',
                          if (packet.window != null) 'win ${packet.window}',
                        ].join(' · '),
                        style: mono,
                      ),
                  ],
                ),
              ),
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 8),
                child: Text(l.captureExchangeCount(packets.length), style: t.typography.caption),
              ),
              for (final item in packets)
                ListTile(
                  dense: true,
                  selected: identical(item, packet),
                  onTap: () => select(item),
                  leading: Icon(item.source == packets.first.source ? Icons.north_east : Icons.south_west, size: 14),
                  title: Text('${item.source} → ${item.destination}', style: mono),
                  subtitle: Text(
                    [packetTime(item), item.summary].where((s) => s.isNotEmpty).join(' · '),
                    style: t.typography.caption,
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}
