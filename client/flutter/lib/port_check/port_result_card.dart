import 'dart:io';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/port_check/port_web_preview.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as ssh;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

typedef PortSocket = ({String? hostId, int port, bool udp});
final portOwnersProvider = FutureProvider.autoDispose.family<List<String>, PortSocket>(
  (ref, socket) => ssh.socketOwners(hostId: socket.hostId, port: socket.port, udp: socket.udp),
);

String? _usualService(int port, bool udp) {
  if (udp) return const {53: 'DNS', 67: 'DHCP', 68: 'DHCP', 123: 'NTP', 161: 'SNMP', 5353: 'mDNS'}[port];
  return const {
    21: 'FTP',
    22: 'SSH',
    25: 'SMTP',
    53: 'DNS',
    80: 'HTTP',
    110: 'POP3',
    143: 'IMAP',
    443: 'HTTPS',
    465: 'SMTPS',
    587: 'SMTP',
    993: 'IMAPS',
    995: 'POP3S',
    3306: 'MySQL',
    3389: 'RDP',
    5432: 'PostgreSQL',
    6379: 'Redis',
    6443: 'Kubernetes API (HTTPS)',
    8080: 'HTTP',
    8443: 'HTTPS',
  }[port];
}

class PortResultCard extends StatelessWidget {
  const PortResultCard({
    super.key,
    required this.number,
    required this.udp,
    required this.host,
    required this.sourceId,
    required this.sourceLabel,
    required this.expanded,
    required this.onToggle,
    required this.status,
    required this.statusLabel,
    required this.socket,
    required this.onRetry,
  });

  final int number;
  final bool udp, expanded;
  final String host, sourceLabel;
  final String? sourceId, status, statusLabel;
  final VoidCallback? onToggle, onRetry;
  final PortSocket? socket;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final t = GlassTokens.of(context);
    final service = _usualService(number, udp);
    return ContentSurface(
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Semantics(
            expanded: expanded,
            child: ListTile(
              onTap: onToggle,
              leading: Icon(
                udp ? Icons.swap_horiz_rounded : Icons.settings_ethernet_rounded,
                size: 18,
                color: t.secondaryLabel,
              ),
              title: Text('$number/${udp ? 'UDP' : 'TCP'}', style: t.typography.bodyEmph),
              subtitle: statusLabel == null ? null : Text(statusLabel!),
              trailing: Icon(expanded ? Icons.expand_less : Icons.expand_more),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Divider(),
                  Text('$sourceLabel  →  $host', style: t.typography.mono),
                  if (service != null) Text(l.portTypicalService(service)),
                  if (socket case final target?) _SocketOwner(socket: target),
                  if (status == 'refused') Text(l.portReasonRefused),
                  if (status == 'timeout' || status == 'unreachable') Text(l.portReasonUnreachable),
                  if (!udp && status == 'connected')
                    _WebsiteDetails(
                      host: host,
                      port: number,
                      sourceId: sourceId,
                      inspectUnknown: socket != null && service == null,
                    ),
                  if (onRetry != null)
                    GlassButton(icon: Icons.refresh_rounded, label: l.portCheckAction, onPressed: onRetry),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _SocketOwner extends ConsumerWidget {
  const _SocketOwner({required this.socket});
  final PortSocket socket;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final result = ref.watch(portOwnersProvider(socket));
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Text(
        result.when(
          data: (names) => names.isEmpty ? l.portOwnerUnavailable : l.portOwner(names.join(', ')),
          error: (_, _) => l.portOwnerUnavailable,
          loading: () => l.portOwnerReading,
        ),
      ),
    );
  }
}

class _WebsiteDetails extends ConsumerStatefulWidget {
  const _WebsiteDetails({required this.host, required this.port, required this.sourceId, required this.inspectUnknown});
  final String host;
  final int port;
  final String? sourceId;
  final bool inspectUnknown;
  @override
  ConsumerState<_WebsiteDetails> createState() => _WebsiteDetailsState();
}

class _WebsiteDetailsState extends ConsumerState<_WebsiteDetails> {
  late String? scheme = switch (widget.port) {
    80 || 8080 => 'http',
    443 || 6443 || 8443 => 'https',
    _ => widget.inspectUnknown ? 'http' : null,
  };
  bool openFailed = false;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final bare = widget.host.startsWith('[') && widget.host.endsWith(']')
        ? widget.host.substring(1, widget.host.length - 1)
        : widget.host;
    final loopback =
        InternetAddress.tryParse(bare)?.isLoopback == true ||
        bare.toLowerCase().replaceFirst(RegExp(r'\.$'), '') == 'localhost';
    final origin = scheme == null
        ? null
        : (sourceId: widget.sourceId, uri: Uri(scheme: scheme, host: bare, port: widget.port, path: '/'));
    final preview = origin == null ? null : ref.watch(portWebPageProvider(origin));
    final page = preview?.value;
    final problem = page?.error;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(l.portWebsite, style: GlassTokens.of(context).typography.bodyEmph),
          const SizedBox(height: 6),
          Wrap(
            spacing: 6,
            runSpacing: 4,
            children: [
              for (final protocol in ['http', 'https'])
                GlassButton(
                  label: protocol.toUpperCase(),
                  onPressed: () {
                    if (scheme == protocol && origin != null) ref.invalidate(portWebPageProvider(origin));
                    setState(() {
                      scheme = protocol;
                      openFailed = false;
                    });
                  },
                ),
            ],
          ),
          if (preview?.isLoading == true) Text(l.portWebsiteReading),
          if (problem != null || preview?.hasError == true)
            Text(switch (problem) {
              'certificate' || 'tls' => l.portWebsiteTls,
              'timeout' => l.portCheckTimeout,
              'no_python' => l.portCheckPython,
              _ => l.portWebsiteUnavailable,
            }),
          if (origin != null && page?.status != null) ...[
            const SizedBox(height: 6),
            Text('${origin.uri.scheme.toUpperCase()} ${page!.status}${page.mime.isEmpty ? '' : ' · ${page.mime}'}'),
            if (page.title.isNotEmpty) Text(page.title, style: GlassTokens.of(context).typography.bodyEmph),
            if (page.description.isNotEmpty) Text(page.description),
            if (widget.sourceId != null && loopback)
              Text(l.portRemoteLoopback)
            else ...[
              Tooltip(
                message: l.wslcOpenInBrowser,
                child: GlassButton(
                  icon: Icons.open_in_new_rounded,
                  label: origin.uri.toString(),
                  onPressed: () async {
                    bool opened;
                    try {
                      opened = await ref.read(portBrowserProvider)(origin.uri);
                    } catch (_) {
                      opened = false;
                    }
                    if (mounted && scheme == origin.uri.scheme) setState(() => openFailed = !opened);
                  },
                ),
              ),
              if (openFailed) ...[SelectableText(origin.uri.toString()), Text(l.portBrowserFailed)],
            ],
          ],
        ],
      ),
    );
  }
}
