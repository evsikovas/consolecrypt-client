import 'dart:io';

/// Decode packet headers from tcpdump output. Linux cooked capture
/// can also print an interface and direction before the network header.
final class CapturePacket {
  CapturePacket(
    this.time,
    this.protocol,
    this.source,
    this._destination,
    this.summary, {
    required this.raw,
    this.frameLength,
    this.flags = '',
    this.network = '',
    this.interface,
    this.direction,
    this.sequence,
    this.ack,
    this.window,
    this.length,
  });
  final DateTime time;
  final String protocol, source, summary, raw, flags;
  String get destination {
    if (_destination.isNotEmpty) return _destination;
    // ARP replies carry the target IPv4 address in the packet body.
    if (protocol == 'ARP' &&
        _bytes.length >= 28 &&
        _bytes[0] == 0 &&
        _bytes[1] == 1 &&
        _bytes[2] == 8 &&
        _bytes[3] == 0 &&
        _bytes[4] == 6 &&
        _bytes[5] == 4) {
      return _bytes.sublist(24, 28).join('.');
    }
    return '—';
  }

  final String _destination;
  final int? frameLength;
  final _bytes = <int>[];
  static final _hexLine = RegExp(r'^0x([0-9a-fA-F]{4}):\s+(.*)$');
  static final _group = RegExp(r'^(?:[0-9a-fA-F]{2}){1,2}$');

  bool appendHex(String line) {
    final m = _hexLine.firstMatch(line);
    if (m == null || int.parse(m[1]!, radix: 16) != _bytes.length) return false;
    final words = m[2]!.trimLeft().split(RegExp(r'\s{2,}')).first.split(' ');
    if (words.any((word) => !_group.hasMatch(word))) return false;
    final row = words.join();
    if (row.length > 32 || _bytes.length + row.length ~/ 2 > 96) return false;
    for (var i = 0; i < row.length; i += 2) {
      _bytes.add(int.parse(row.substring(i, i + 2), radix: 16));
    }
    return true;
  }

  String get hexDump {
    final rows = <String>[];
    for (var i = 0; i < _bytes.length; i += 16) {
      final row = _bytes.skip(i).take(16);
      final hex = row.map((b) => b.toRadixString(16).padLeft(2, '0')).join(' ');
      final ascii = String.fromCharCodes(row.map((b) => b >= 32 && b < 127 ? b : 46));
      rows.add('${i.toRadixString(16).padLeft(4, '0')}  ${hex.padRight(47)}  $ascii');
    }
    return rows.join('\n');
  }

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

  static final _linkHeader = RegExp(
    r'^(\d{9,11})\.(\d{1,9})\s+(.*?)ethertype (IPv4|IPv6|ARP) \((0x[0-9a-fA-F]+)\), length (\d+):\s*(.*)$',
  );
  static final _direction = RegExp(r'^(\S+)\s+(In|Out|B|M|P)\s');
  static DateTime _time(String seconds, String fraction) => DateTime.fromMicrosecondsSinceEpoch(
    int.parse(seconds) * 1000000 + int.parse(fraction.padRight(6, '0').substring(0, 6)),
  );

  static CapturePacket? parse(String line) {
    final link = _linkHeader.firstMatch(line);
    var header = line;
    if (link != null) {
      final iface = _direction.firstMatch(link[3]!);
      final detail = link[7]!;
      if (link[4] == 'ARP') {
        final request = RegExp(r'^Request who-has (\S+).*? tell ([^, ]+)').firstMatch(detail);
        final reply = RegExp(r'^Reply (\S+) is-at ([^, ]+)').firstMatch(detail);
        if (request == null && reply == null) return null;
        return CapturePacket(
          _time(link[1]!, link[2]!),
          'ARP',
          request?[2] ?? reply![1]!,
          request?[1] ?? '',
          detail,
          raw: line,
          network: 'ARP',
          frameLength: int.parse(link[6]!),
          interface: iface?[1],
          direction: iface?[2],
        );
      }
      final prefix = iface == null ? '' : '${iface[1]} ${iface[2]} ';
      header = '${link[1]}.${link[2]} $prefix${link[4] == 'IPv6' ? 'IP6' : 'IP'} $detail';
    }
    final m = _packet.firstMatch(header);
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
    return CapturePacket(
      _time(m[1]!, m[2]!),
      protocol,
      endpoint(m[6]!),
      endpoint(m[7]!),
      summary,
      raw: line,
      flags: flags ?? '',
      frameLength: int.tryParse(link?[6] ?? ''),
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
