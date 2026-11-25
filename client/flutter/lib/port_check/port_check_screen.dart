import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as ssh;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

typedef CheckPort = Future<String> Function({
  String? sourceId,
  required String host,
  required int port,
  required bool udp,
});
final checkPortProvider = Provider<CheckPort>(
  (ref) =>
      ({sourceId, required host, required port, required udp}) =>
          guard(() => ssh.portCheck(sourceId: sourceId, host: host, port: port, udp: udp)),
);

bool validPortHost(String host) {
  if (InternetAddress.tryParse(host) != null) return true;
  if (host.startsWith('[') && host.endsWith(']')) {
    return InternetAddress.tryParse(host.substring(1, host.length - 1))?.type == InternetAddressType.IPv6;
  }
  final name = host.endsWith('.') ? host.substring(0, host.length - 1) : host;
  final label = RegExp(r'^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$');
  return name.length <= 253 && !RegExp(r'^[0-9.]+$').hasMatch(name) && name.split('.').every(label.hasMatch);
}

String _statusText(AppLocalizations l, String status) => switch (status) {
  'connected' => l.portCheckConnected,
  'reply' => l.portCheckReply,
  'refused' => l.portCheckRefused,
  'no_reply' => l.portCheckNoReply,
  'timeout' => l.portCheckTimeout,
  'dns_error' => l.portCheckDns,
  'unreachable' => l.portCheckUnreachable,
  'no_python' => l.portCheckPython,
  'socket_error' => l.portCheckSocketError,
  _ => l.portCheckRemoteError,
};

class PortCheckScreen extends ConsumerStatefulWidget {
  const PortCheckScreen({super.key});
  @override
  ConsumerState<PortCheckScreen> createState() => _PortCheckScreenState();
}

class _PortCheckScreenState extends ConsumerState<PortCheckScreen> {
  final host = TextEditingController();
  final port = TextEditingController(text: '443');
  Host? source;
  bool remote = false, udp = false, busy = false, validated = false;
  int generation = 0;
  String? status;
  Object? error;

  @override
  void dispose() {
    host.dispose();
    port.dispose();
    super.dispose();
  }

  void clearResult() {
    status = null;
    error = null;
    validated = false;
  }

  void clearProfile() => setState(() {
    generation++;
    busy = remote = udp = false;
    source = null;
    host.clear();
    port.text = '443';
    clearResult();
  });

  Future<void> chooseSource() async {
    final current = generation;
    final chosen = await showHostPicker(context);
    if (mounted && current == generation && chosen != null && !busy) {
      setState(() {
        source = chosen;
        clearResult();
      });
    }
  }

  Future<void> check() async {
    if (busy || remote && source == null) return;
    final address = host.text.trim(), number = int.tryParse(port.text.trim());
    setState(() {
      clearResult();
      validated = true;
    });
    if (!validPortHost(address) || number == null || number < 1 || number > 65535) return;
    final current = ++generation;
    setState(() => busy = true);
    try {
      final result = await ref.read(checkPortProvider)(
        sourceId: remote ? source!.id.value : null,
        host: address,
        port: number,
        udp: udp,
      );
      if (mounted && generation == current) setState(() => status = result);
    } catch (e) {
      if (mounted && generation == current) setState(() => error = e);
    } finally {
      if (mounted && generation == current) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(activeProfileProvider.select((p) => p?.id), (_, _) => clearProfile());
    ref.listen(appStageProvider, (_, stage) {
      if (stage != AppStage.unlocked) clearProfile();
    });
    final l = context.l10n;
    final number = int.tryParse(port.text.trim()) ?? 0;
    return PageScaffold(
      title: l.portCheckTitle,
      subtitle: l.portCheckSubtitle,
      scrollable: true,
      maxWidth: 760,
      body: GlassPanel(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.portCheckSource),
            const SizedBox(height: 8),
            Wrap(
              spacing: 12,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                GlassSegmented<bool>(
                  selected: remote,
                  inChrome: false,
                  semanticLabel: l.portCheckSource,
                  segments: [
                    GlassSegment(value: false, label: l.portCheckLocal),
                    GlassSegment(value: true, label: l.portCheckRemote),
                  ],
                  onChanged: busy
                      ? null
                      : (v) => setState(() {
                          remote = v;
                          clearResult();
                        }),
                ),
                if (remote)
                  GlassButton(
                    icon: Icons.dns_rounded,
                    label: source?.name ?? l.portCheckChoose,
                    onPressed: busy ? null : chooseSource,
                  ),
              ],
            ),
            if (remote) Padding(padding: const EdgeInsets.only(top: 8), child: Text(l.portCheckRemoteHelp)),
            const SizedBox(height: 20),
            Text(l.portCheckTarget),
            const SizedBox(height: 6),
            GlassField(
              fieldKey: const ValueKey('port-check-host'),
              controller: host,
              enabled: !busy,
              semanticLabel: l.portCheckTarget,
              placeholder: 'localhost',
              errorText: validated && !validPortHost(host.text.trim()) ? l.portCheckBadHost : null,
              onChanged: (_) => setState(clearResult),
              onSubmitted: (_) => check(),
            ),
            const SizedBox(height: 16),
            Wrap(
              spacing: 20,
              runSpacing: 12,
              children: [
                SizedBox(
                  width: 160,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(l.portCheckPort),
                      const SizedBox(height: 6),
                      GlassField(
                        fieldKey: const ValueKey('port-check-number'),
                        controller: port,
                        enabled: !busy,
                        keyboardType: TextInputType.number,
                        semanticLabel: l.portCheckPort,
                        errorText: validated && (number < 1 || number > 65535) ? l.portCheckPort : null,
                        onChanged: (_) => setState(clearResult),
                        onSubmitted: (_) => check(),
                      ),
                    ],
                  ),
                ),
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(l.portCheckProtocol),
                    const SizedBox(height: 6),
                    GlassSegmented<bool>(
                      selected: udp,
                      semanticLabel: l.portCheckProtocol,
                      inChrome: false,
                      segments: const [
                        GlassSegment(value: false, label: 'TCP'),
                        GlassSegment(value: true, label: 'UDP'),
                      ],
                      onChanged: busy
                          ? null
                          : (v) => setState(() {
                              udp = v;
                              clearResult();
                            }),
                    ),
                  ],
                ),
              ],
            ),
            if (udp) Padding(padding: const EdgeInsets.only(top: 12), child: Text(l.portCheckUdpHelp)),
            const SizedBox(height: 20),
            GlassButton.prominent(
              key: const ValueKey('port-check-submit'),
              icon: Icons.settings_ethernet_rounded,
              label: busy ? l.portCheckRunning : l.portCheckAction,
              onPressed: busy || remote && source == null ? null : check,
            ),
            if (status != null || error != null)
              Padding(
                padding: const EdgeInsets.only(top: 16),
                child: Semantics(
                  liveRegion: true,
                  child: Text(
                    error != null ? errorMessage(l, error!) : _statusText(l, status!),
                    style: GlassTokens.of(context).typography.bodyEmph,
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
