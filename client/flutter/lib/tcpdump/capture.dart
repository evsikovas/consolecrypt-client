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

String checkedPeer(String address) {
  var peer = address.trim();
  if (peer.startsWith('[') && peer.endsWith(']')) peer = peer.substring(1, peer.length - 1);
  if (peer.isEmpty || peer.length > 253) throw const FormatException('Invalid host');
  if (InternetAddress.tryParse(peer) != null && !peer.contains('%')) return peer;
  // libpcap interprets abbreviated dotted IPv4 as a network prefix.
  if (RegExp(r'^[0-9.]+$').hasMatch(peer)) throw const FormatException('Invalid IP address');
  if (!RegExp(r'^(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)*[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.?$')
      .hasMatch(peer)) {
    throw const FormatException('Invalid host');
  }
  return peer;
}

const captureLimit = 2000;
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

const _detectTcpdump = r'''
export LC_ALL=C PATH="$PATH:/usr/sbin:/sbin";
if ! command -v tcpdump >/dev/null 2>&1; then
  for manager in apt-get dnf yum apk pacman zypper pkg brew; do
    if command -v "$manager" >/dev/null 2>&1; then echo "CC_MISSING $manager $(id -u)"; exit 43; fi;
  done;
  echo "CC_MISSING unknown"; exit 43;
fi;
''';

String _shellCommand(String script) {
  final line = script.replaceAll('\n', ' ').trim();
  return "exec sh -c '${line.replaceAll("'", "'\\''")}'";
}

String get remoteTcpdumpCheck => _shellCommand('$_detectTcpdump echo CC_TCPDUMP_READY');

/// Run in the foreground in a dedicated SSH PTY. Closing the PTY hangs up
/// the capture even when it is waiting for its first packet.
String remoteCaptureCommand(String destination, {bool privileged = false}) {
  final peer = checkedPeer(destination);
  const preflight = r'''
set -f; set -- $SSH_CONNECTION;
if [ "$#" -ne 4 ]; then echo CC_NO_PEER; exit 44; fi;
for ip in "$1" "$3"; do case "$ip" in ""|*[!a-fA-F0-9:.]*) echo CC_NO_PEER; exit 44;; esac; done;
for port in "$2" "$4"; do case "$port" in ""|*[!0-9]*) echo CC_NO_PEER; exit 44;; esac; done;
exclude="not (tcp and ((src host $1 and src port $2 and dst host $3 and dst port $4) or (src host $3 and src port $4 and dst host $1 and dst port $2)))";
''';
  const localAddresses = r'''
if command -v ip >/dev/null 2>&1; then
  addrs=$(ip -o addr show | awk '$3=="inet" || $3=="inet6" {split($4,a,"/"); print a[1]}');
else
  addrs=$(ifconfig -a | awk '$1=="inet" || $1=="inet6" {split($2,a,"%"); print a[1]}');
fi;
sources=""; destinations="";
for addr in $addrs; do case "$addr" in ""|*[!a-fA-F0-9:.]*) continue;; esac;
  sources="${sources:+$sources or }src host $addr"; destinations="${destinations:+$destinations or }dst host $addr";
done;
if [ -z "$sources" ]; then echo CC_NO_PEER; exit 44; fi;
''';
  final pair = '$localAddresses pair="(dst host $peer and (\$sources)) or (src host $peer and (\$destinations))";';
  const capture = r'''
set --; if [ "$(uname -s)" = Linux ]; then set -- -i any; fi;
''';
  final elevation = privileged ? r'prefix=; if [ "$(id -u)" != 0 ]; then prefix="sudo -n"; fi;' : '';
  return _shellCommand(
    '$_detectTcpdump $preflight $pair $capture $elevation exec ${privileged ? r'$prefix ' : ''}tcpdump -p -nn -tt -l -s 96 -c $captureLimit "\$@" "(ip or ip6) and (\$pair) and \$exclude" 2>&1',
  );
}
