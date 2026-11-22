import 'dart:io';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// A container port as shown by `container list` or Inspect.
@immutable
class PublishedPort {
  const PublishedPort({required this.containerPort, this.hostIp, this.hostPort});

  final String containerPort;
  final String? hostIp;
  final int? hostPort;

  factory PublishedPort.fromBinding(PortBinding b) =>
      PublishedPort(containerPort: b.containerPort, hostIp: b.hostIp, hostPort: int.tryParse(b.hostPort ?? ''));

  bool get published => hostPort != null && hostPort! > 0;
  bool get tcp => !containerPort.toLowerCase().endsWith('/udp');
  bool get https => containerPort.startsWith('443') || hostPort == 443;

  /// Bound to a loopback address: reachable only from this computer.
  bool get loopbackOnly => const {'127.0.0.1', '::1', 'localhost'}.contains(hostIp);

  String get label => published ? '$hostPort → $containerPort' : containerPort;

  /// Docker-style `host:container` for narrow columns.
  String get shortLabel => published ? '$hostPort:${containerPort.split('/').first}' : containerPort;

  String url(String host) {
    final h = host.contains(':') ? '[$host]' : host;
    return '${https ? 'https' : 'http'}://$h:$hostPort';
  }

  @override
  bool operator ==(Object other) =>
      other is PublishedPort &&
      other.containerPort == containerPort &&
      other.hostIp == hostIp &&
      other.hostPort == hostPort;

  @override
  int get hashCode => Object.hash(containerPort, hostIp, hostPort);
}

final _mapping = RegExp(r'^(?:(.*):)?(\d+)->(\d+(?:/\w+)?)$');
final _exposed = RegExp(r'^\d+(?:/\w+)?$');

/// Parses `127.0.0.1:18081->80/tcp, 0.0.0.0:53->53/udp`. Unknown parts are skipped.
List<PublishedPort> parsePorts(String? raw) {
  if (raw == null) return const [];
  final result = <PublishedPort>[];
  for (final part in raw.split(',')) {
    final text = part.trim();
    final m = _mapping.firstMatch(text);
    if (m != null) {
      var ip = m.group(1);
      if (ip != null && ip.startsWith('[') && ip.endsWith(']')) {
        ip = ip.substring(1, ip.length - 1);
      }
      result.add(
        PublishedPort(
          containerPort: m.group(3)!,
          hostIp: ip == null || ip.isEmpty ? null : ip,
          hostPort: int.parse(m.group(2)!),
        ),
      );
    } else if (_exposed.hasMatch(text)) {
      result.add(PublishedPort(containerPort: text));
    }
  }
  return result;
}

@immutable
class HostAddress {
  const HostAddress(this.interface, this.address);
  final String interface;
  final String address;
}

/// IPv4 addresses of this computer, physical adapters first.
Future<List<HostAddress>> loadHostAddresses() async {
  final interfaces = await NetworkInterface.list(type: InternetAddressType.IPv4);
  final result = [
    for (final i in interfaces)
      for (final a in i.addresses)
        if (!a.isLoopback && !a.isLinkLocal) HostAddress(i.name, a.address),
  ];
  result.sort((a, b) => addressRank(a) - addressRank(b));
  return result.take(4).toList();
}

final _wireless = RegExp(r'Wi-?Fi|WLAN|Wireless|Беспровод', caseSensitive: false);
final _virtual = RegExp(
  r'vEthernet|WSL|Hyper-V|VirtualBox|VMware|Loopback|Docker|VPN|WireGuard|'
  r'Tailscale|ZeroTier|Amnezia|TAP|TUN',
  caseSensitive: false,
);

/// Lower is better: real Wi-Fi/Ethernet adapters first, virtual adapters and VPNs last.
/// Windows names the VirtualBox host-only adapter "Ethernet", so its default
/// 192.168.56.0/24 network is ranked as virtual too.
int addressRank(HostAddress a) {
  if (_virtual.hasMatch(a.interface) || a.address.startsWith('192.168.56.')) {
    return 3;
  }
  if (_wireless.hasMatch(a.interface)) return 0;
  return RegExp(r'Ethernet|LAN|локальн', caseSensitive: false).hasMatch(a.interface) ? 1 : 2;
}

/// Reports the type of content copied without exposing its value in the toast.
Future<void> copyText(BuildContext context, String text, {String? label}) async {
  final notify = WslcApiScope.notifierOf(context);
  final l = context.l10n;
  try {
    await WslcApiScope.copy(context, ClipboardData(text: text));
  } catch (_) {
    notify(l.clipboardSecureCopyFailed, error: true);
    return;
  }
  notify(l.copiedNotice(label ?? l.copyWhatText));
}

Future<void> openPort(BuildContext context, PublishedPort port, String host) async {
  final notify = WslcApiScope.notifierOf(context);
  try {
    await WslcApiScope.of(context).openPortUrl(host: host, port: port.hostPort!, https: port.https);
  } catch (e) {
    notify(errorText(e), error: true);
  }
}

/// Clickable port chip: open in the browser or copy, via localhost or this computer's IP.
class PortChip extends StatelessWidget {
  const PortChip({super.key, required this.port, required this.addresses, this.dense = false});
  final PublishedPort port;
  final List<HostAddress> addresses;
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final hostPort = port.hostPort;
    final active = hostPort != null && hostPort > 0;
    final chip = Container(
      padding: EdgeInsets.symmetric(horizontal: dense ? 7 : 10, vertical: 3),
      decoration: BoxDecoration(
        color: (active ? p.accentEnd : p.faint).withValues(alpha: .12),
        borderRadius: BorderRadius.circular(6),
        border: Border.all(color: (active ? p.accentEnd : p.faint).withValues(alpha: .28)),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Flexible(
            child: Text(
              dense ? port.shortLabel : port.label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontFamily: monoFont,
                fontFamilyFallback: monoFallback,
                fontSize: dense ? 11.5 : 12.5,
                fontWeight: FontWeight.w600,
                color: active ? p.accentEnd : p.muted,
              ),
            ),
          ),
          if (active && !dense) ...[
            const SizedBox(width: 4),
            Icon(Icons.expand_more_rounded, size: 14, color: p.accentEnd),
          ],
        ],
      ),
    );
    if (hostPort == null || hostPort <= 0) {
      return Tooltip(message: context.l10n.wslcPortIsNotPublishedOnThisComputer(port.containerPort), child: chip);
    }
    const local = 'localhost';
    final reachableByIp = !port.loopbackOnly;
    return PopupMenuButton<VoidCallback>(
      tooltip: context.l10n.wslcOpenOrCopyPortAddress,
      position: PopupMenuPosition.under,
      onSelected: (action) => action(),
      itemBuilder: (context) => [
        if (port.tcp)
          _item(
            Icons.open_in_browser_rounded,
            context.l10n.wslcOpen(port.url(local)),
            () => openPort(context, port, local),
          ),
        if (port.tcp && reachableByIp)
          for (final a in addresses)
            _item(
              Icons.lan_outlined,
              context.l10n.wslcOpen(port.url(a.address)),
              () => openPort(context, port, a.address),
              hint: a.interface,
            ),
        if (port.tcp) const PopupMenuDivider(),
        _item(
          Icons.content_copy_rounded,
          context.l10n.wslcCopyPorts(port.url(local)),
          () => copyText(context, port.url(local)),
        ),
        _item(
          Icons.content_copy_rounded,
          context.l10n.wslcCopyLocalhost(hostPort),
          () => copyText(context, 'localhost:${port.hostPort}'),
        ),
        if (reachableByIp)
          for (final a in addresses)
            _item(
              Icons.content_copy_rounded,
              context.l10n.wslcCopyPortsOther(a.address, hostPort),
              () => copyText(context, '${a.address}:${port.hostPort}'),
              hint: a.interface,
            )
        else ...[
          const PopupMenuDivider(),
          PopupMenuItem<VoidCallback>(
            enabled: false,
            child: SizedBox(
              width: 300,
              child: Text(
                context.l10n.wslcThisPortIsPublishedOnlyOnAndCannot(
                  port.hostIp ?? local,
                  hostPort,
                  port.containerPort.split('/').first,
                ),
                style: TextStyle(fontSize: 12, color: p.muted),
              ),
            ),
          ),
        ],
      ],
      child: chip,
    );
  }

  PopupMenuItem<VoidCallback> _item(IconData icon, String text, VoidCallback action, {String? hint}) =>
      PopupMenuItem<VoidCallback>(
        value: action,
        height: 40,
        child: Row(
          children: [
            Icon(icon, size: 18),
            const SizedBox(width: 12),
            Flexible(child: Text(text, overflow: TextOverflow.ellipsis)),
            if (hint != null) ...[
              const SizedBox(width: 8),
              Text(
                hint,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(fontSize: 11.5, color: Colors.grey),
              ),
            ],
          ],
        ),
      );
}

/// Compact list of port chips with a "+N" overflow badge.
class PortChips extends StatelessWidget {
  const PortChips({super.key, required this.ports, required this.addresses, this.max = 2, this.dense = true});
  final List<PublishedPort> ports;
  final List<HostAddress> addresses;
  final int max;
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final shown = ports.take(max).toList();
    return Wrap(
      spacing: 4,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final port in shown) PortChip(port: port, addresses: addresses, dense: dense),
        if (ports.length > max)
          Tooltip(
            message: ports.skip(max).map((p) => p.label).join('\n'),
            child: Text('+${ports.length - max}', style: TextStyle(fontSize: 12, color: DeskPalette.of(context).muted)),
          ),
      ],
    );
  }
}
