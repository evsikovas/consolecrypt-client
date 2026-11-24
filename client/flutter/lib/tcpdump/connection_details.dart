import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:material_ui/material_ui.dart';

String packetTime(CapturePacket packet) => packet.time.toLocal().toIso8601String().split('T').last.substring(0, 12);

class CaptureConnectionDetails extends StatelessWidget {
  const CaptureConnectionDetails({required this.packet, required this.count, required this.back, super.key});
  final CapturePacket packet;
  final int count;
  final VoidCallback back;

  @override
  Widget build(BuildContext context) {
    final t = GlassTokens.of(context), l = context.l10n;
    final mono = t.typography.mono.copyWith(fontSize: 11);
    final flags = packet.flags;
    final explanation = switch (packet.protocol) {
      'TCP' when flags.contains('R') => l.captureRst,
      'TCP' when flags.contains('S') => flags.contains('.') ? l.captureSynAck : l.captureSyn,
      'TCP' when flags.contains('F') => l.captureFin,
      'TCP' when (packet.length ?? 0) > 0 => l.captureData,
      'TCP' when flags.contains('.') => l.captureAck,
      'ARP' => packet.summary.startsWith('Request') ? l.captureArpQuery : l.captureArpReply,
      _ => packet.summary,
    };
    return GlassPanel(
      padding: const EdgeInsets.fromLTRB(12, 6, 12, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              GlassIconButton(icon: Icons.arrow_back_rounded, tooltip: l.commonBack, onPressed: back, size: 28),
              const SizedBox(width: 8),
              Expanded(child: Text(l.captureExchange, style: t.typography.bodyEmph)),
            ],
          ),
          SelectableText('${packet.source} → ${packet.destination}', style: t.typography.bodyEmph),
          Text(
            [
              packet.time.toLocal().toIso8601String().replaceFirst('T', ' '),
              packet.network,
              if (packet.network != packet.protocol) packet.protocol,
              if (packet.interface != null) [packet.interface, ?packet.direction].join(' '),
              if (packet.frameLength != null) '${packet.frameLength} B',
            ].join(' · '),
            style: t.typography.caption,
          ),
          if (explanation.isNotEmpty) Text(explanation),
          if (packet.sequence != null || packet.ack != null || packet.window != null)
            SelectableText(
              [
                if (packet.sequence != null) 'seq ${packet.sequence}',
                if (packet.ack != null) 'ack ${packet.ack}',
                if (packet.window != null) 'win ${packet.window}',
              ].join(' · '),
              style: mono,
            ),
          SelectableText(packet.raw, style: mono),
          if (packet.hexDump.isNotEmpty) ...[
            Text(l.capturePacketBytes, style: t.typography.caption),
            SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: SelectableText(packet.hexDump, style: mono),
            ),
          ],
          Text(l.captureExchangeCount(count), style: t.typography.caption),
        ],
      ),
    );
  }
}
