import 'dart:io';

/// Decode packet headers from tcpdump output. Linux cooked capture
/// can also print an interface and direction before the network header.
final class CapturePacket {
  const CapturePacket(
    this.time,
    this.protocol,
    this.source,
    this.destination,
    this.summary, {
    this.network = '',
    this.interface,
    this.direction,
    this.sequence,
    this.ack,
    this.window,
    this.length,
  });
  final DateTime time;
  final String protocol, source, destination, summary;

  final String network;
  final String? interface, direction, sequence, ack, window;
  final int? length;

  String get transport => protocol == 'DNS' ? 'UDP' : protocol;
  bool sharesEndpoints(CapturePacket other) =>
      transport == other.transport &&
      ((source == other.source && destination == other.destination) ||
          (source == other.destination && destination == other.source));

  static final _packet = RegExp(
    r'^(\d{9,11})\.(\d{1,9})\s+(?:(?<iface>\S+)\s+(?<direction>In|Out|B|M|P)\s+)?(IP6?)\s+(\S+) > (\S+):\s*(.*)$',
  );
  static final _port = RegExp(r'^(.+)\.(\d{1,5})$');

  static String endpoint(String input) {
    final m = _port.firstMatch(input);
    if (m == null || InternetAddress.tryParse(m[1]!) == null) return input;
    return m[1]!.contains(':') ? '[${m[1]}]:${m[2]}' : '${m[1]}:${m[2]}';
  }

  static CapturePacket? parse(String line) {
    final m = _packet.firstMatch(line);
    if (m == null) return null;
    final detail = m[8]!;
    final flags = RegExp(r'^Flags \[([FSRP.UWE0-9]+)\]').firstMatch(detail)?[1];
    final tcp = flags != null || detail.startsWith('tcp ');
    final length = RegExp(r'\blength (\d+)').allMatches(detail).lastOrNull?[1];
    final dnsPort = m[6]!.endsWith('.53') || m[7]!.endsWith('.53');
    final dns = dnsPort && RegExp(r'^\d+[+*%|!-]*\s').hasMatch(detail);
    final icmp = detail.startsWith('ICMP');
    final protocol = switch (detail) {
      _ when tcp => 'TCP',
      _ when dns => 'DNS',
      _ when icmp => m[5] == 'IP6' ? 'ICMPv6' : 'ICMP',
      _ when detail.startsWith('UDP') => 'UDP',
      _ => m[5] == 'IP6' ? 'IPv6' : 'IPv4',
    };
    const flagLabels = {'S': 'SYN', '.': 'ACK', 'F': 'FIN', 'R': 'RST', 'P': 'PSH', 'U': 'URG', 'W': 'CWR', 'E': 'ECE'};
    final summary = <String>[
      if (flags != null) ...flags.split('').map((f) => flagLabels[f] ?? f),
      if (dns) ...[
        RegExp(r'\b(?:A|AAAA|PTR|TXT|MX|SRV|HTTPS|NS|CNAME)\?').firstMatch(detail)?[0] ?? 'DNS',
        '#${RegExp(r"^\d+").stringMatch(detail)}',
      ],
      if (icmp) detail.split(',').take(2).join(', ').replaceFirst(RegExp(r'^ICMP6?[, ]+'), '').trim(),
      if (length != null) '$length B',
    ].join(' · ');
    final stamp = int.parse(m[1]!) * 1000000 + int.parse(m[2]!.padRight(6, '0').substring(0, 6));
    return CapturePacket(
      DateTime.fromMicrosecondsSinceEpoch(stamp),
      protocol,
      endpoint(m[6]!),
      endpoint(m[7]!),
      summary,
      network: m[5] == 'IP6' ? 'IPv6' : 'IPv4',
      interface: m.namedGroup('iface'),
      direction: m.namedGroup('direction'),
      sequence: RegExp(r'\bseq ([0-9]+(?::[0-9]+)?)').firstMatch(detail)?[1],
      ack: tcp ? (RegExp(r'\back (\d+)').firstMatch(detail)?[1]) : null,
      window: tcp ? (RegExp(r'\bwin (\d+)').firstMatch(detail)?[1]) : null,
      length: int.tryParse(length ?? ''),
    );
  }
}

const displayLimit = 300;
const installCommands = {
  'apt-get': 'sudo apt-get install tcpdump',
  'dnf': 'sudo dnf install tcpdump',
  'yum': 'sudo yum install tcpdump',
  'apk': 'sudo apk add tcpdump',
  'pacman': 'sudo pacman -S tcpdump',
  'zypper': 'sudo zypper install tcpdump',
  'pkg': 'sudo pkg install tcpdump',
  'brew': 'brew install tcpdump',
};
