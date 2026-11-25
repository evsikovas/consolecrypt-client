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
import 'package:consolecrypt/port_check/port_result_card.dart';
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

typedef ReadListeningPorts = Future<List<int>> Function({
  String? hostId,
  required int low,
  required int high,
  required bool udp,
});
final listeningPortsProvider = Provider<ReadListeningPorts>(
  (ref) =>
      ({hostId, required low, required high, required udp}) =>
          guard(() => ssh.listeningPorts(hostId: hostId, low: low, high: high, udp: udp)),
);

({int low, int high})? parsePortInterval(String text) {
  final match = RegExp(r'^(\d{1,5})(?:\s*[-–—]\s*(\d{1,5}))?$').firstMatch(text.trim());
  if (match == null) return null;
  final low = int.parse(match[1]!);
  final high = int.parse(match[2] ?? match[1]!);
  return low >= 1 && low <= high && high <= 65535 ? (low: low, high: high) : null;
}

String _bareHost(String value) =>
    value.startsWith('[') && value.endsWith(']') ? value.substring(1, value.length - 1) : value;

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

enum _Activity { resolvingDestination, readingSockets, checkingPort }

class _PortCheckScreenState extends ConsumerState<PortCheckScreen> {
  final host = TextEditingController();
  final port = TextEditingController(text: '443');
  Host? source;
  Host? inventoryHost;
  List<int>? listeners;
  int? expandedPort;
  ({String? hostId})? resultTarget;
  final checked = <int, String>{};
  static const pageSize = 12;
  int offset = 0;
  bool needsHost = false;
  bool remote = false, udp = false, validated = false;
  _Activity? activity;
  int? checkingPort;
  bool get busy => activity != null;
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
    needsHost = false;
    listeners = null;
    expandedPort = null;
    resultTarget = null;
    checked.clear();
    offset = 0;
  }

  void clearProfile() => setState(() {
    generation++;
    activity = null;
    checkingPort = null;
    remote = udp = false;
    source = null;
    inventoryHost = null;
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

  Future<void> chooseDestination() async {
    final current = generation;
    final chosen = await showHostPicker(context, title: context.l10n.portCheckChooseTarget);
    if (!mounted || current != generation || chosen == null || busy) return;
    setState(() {
      clearResult();
      inventoryHost = chosen;
      host.text = chosen.address;
    });
  }

  // Loopback always refers to the source of the connection check.
  Future<({String? hostId})?> inventoryFor(String address) async {
    if (inventoryHost case final selected?) return (hostId: selected.id.value);
    final bare = _bareHost(address).toLowerCase();
    final ip = InternetAddress.tryParse(bare);
    if (ip?.isLoopback == true || bare == 'localhost' || bare == 'localhost.') {
      return (hostId: remote ? source!.id.value : null);
    }
    final saved = await ref.read(hostsProvider.future);
    final matches = saved.where((h) => h.protocol == HostProtocol.ssh && _bareHost(h.address).toLowerCase() == bare);
    if (matches.length == 1) return (hostId: matches.single.id.value);
    if (matches.isNotEmpty) return null;
    if (ip != null) {
      final interfaces = await NetworkInterface.list();
      if (interfaces.any((i) => i.addresses.contains(ip))) return (hostId: null);
    } else if (bare == Platform.localHostname.toLowerCase()) {
      return (hostId: null);
    }
    return null;
  }

  Future<void> checkListener(int number) async {
    if (busy || !(listeners?.contains(number) ?? false)) return;
    final current = ++generation;
    setState(() {
      activity = _Activity.checkingPort;
      checkingPort = number;
      error = null;
    });
    try {
      final result = await ref.read(checkPortProvider)(
        sourceId: remote ? source!.id.value : null,
        host: host.text.trim(),
        port: number,
        udp: udp,
      );
      if (mounted && generation == current) setState(() => checked[number] = result);
    } catch (e) {
      if (mounted && generation == current) setState(() => error = e);
    } finally {
      finishActivity(current);
    }
  }

  void finishActivity(int current) {
    if (mounted && generation == current) {
      setState(() {
        activity = null;
        checkingPort = null;
      });
    }
  }

  Future<void> check() async {
    if (busy || remote && source == null) return;
    final address = host.text.trim(), interval = parsePortInterval(port.text);
    setState(() {
      clearResult();
      validated = true;
    });
    if (!validPortHost(address) || interval == null) return;
    final current = ++generation;
    final range = interval.low != interval.high;
    setState(() {
      activity = range ? _Activity.resolvingDestination : _Activity.checkingPort;
      checkingPort = range ? null : interval.low;
    });
    try {
      if (range) {
        final destination = await inventoryFor(address);
        if (!mounted || generation != current) return;
        if (destination == null) {
          setState(() => needsHost = true);
          return;
        }
        setState(() => activity = _Activity.readingSockets);
        final ports = await ref.read(listeningPortsProvider)(
          hostId: destination.hostId,
          low: interval.low,
          high: interval.high,
          udp: udp,
        );
        if (mounted && generation == current) {
          setState(() {
            listeners = ports;
            resultTarget = destination;
          });
        }
        return;
      }
      final result = await ref.read(checkPortProvider)(
        sourceId: remote ? source!.id.value : null,
        host: address,
        port: interval.low,
        udp: udp,
      );
      if (!mounted || generation != current) return;
      setState(() {
        status = result;
        expandedPort = interval.low;
      });
      // The connection result is usable even when the destination has no local inventory.
      final target = await inventoryFor(address).onError((_, _) => null);
      if (mounted && generation == current) setState(() => resultTarget = target);
    } catch (e) {
      if (mounted && generation == current) setState(() => error = e);
    } finally {
      finishActivity(current);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(activeProfileProvider.select((p) => p?.id), (_, _) => clearProfile());
    ref.listen(appStageProvider, (_, stage) {
      if (stage != AppStage.unlocked) clearProfile();
    });
    final l = context.l10n;
    final interval = parsePortInterval(port.text);
    final range = interval != null && interval.low != interval.high;
    final activityText = switch (activity) {
      _Activity.resolvingDestination => l.portCheckResolving,
      _Activity.readingSockets => l.portCheckReading(udp ? 'UDP' : 'TCP'),
      _Activity.checkingPort => l.portCheckChecking(checkingPort!),
      null => null,
    };
    return PageScaffold(
      title: l.portCheckTitle,
      subtitle: l.portCheckSubtitle,
      scrollable: true,
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GlassPanel(
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
                  trailing: GlassIconButton(
                    key: const ValueKey('port-check-pick-destination'),
                    tooltip: l.portCheckChooseTarget,
                    icon: Icons.dns_rounded,
                    style: GlassIconButtonStyle.plain,
                    onPressed: busy ? null : chooseDestination,
                  ),
                  errorText: needsHost
                      ? l.portCheckNeedsTarget
                      : validated && !validPortHost(host.text.trim())
                      ? l.portCheckBadHost
                      : null,
                  onChanged: (_) => setState(() {
                    inventoryHost = null;
                    clearResult();
                  }),
                  onSubmitted: (_) => check(),
                ),
                const SizedBox(height: 16),
                Wrap(
                  spacing: 20,
                  runSpacing: 12,
                  children: [
                    SizedBox(
                      width: 200,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(l.portCheckPort),
                          const SizedBox(height: 6),
                          GlassField(
                            fieldKey: const ValueKey('port-check-number'),
                            controller: port,
                            enabled: !busy,
                            keyboardType: TextInputType.text,
                            placeholder: '443 / 22-10000',
                            semanticLabel: l.portCheckPort,
                            errorText: validated && interval == null ? l.portCheckInvalidInterval : null,
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
                if (range) ...[const SizedBox(height: 12), Text(l.portCheckRangeHelp)],
                if (udp) Padding(padding: const EdgeInsets.only(top: 12), child: Text(l.portCheckUdpHelp)),
                const SizedBox(height: 20),
                GlassButton.prominent(
                  key: const ValueKey('port-check-submit'),
                  icon: Icons.settings_ethernet_rounded,
                  label: range ? l.portCheckList : l.portCheckAction,
                  onPressed: busy || remote && source == null ? null : check,
                ),
                if (activityText != null) ...[
                  const SizedBox(height: 12),
                  Semantics(liveRegion: true, child: Text(activityText)),
                  const SizedBox(height: 6),
                  const LinearProgressIndicator(minHeight: 3),
                ],
              ],
            ),
          ),
          if (listeners case final ports?) ...[
            const SizedBox(height: 16),
            Text(ports.isEmpty ? l.portCheckEmpty : l.portCheckFound(ports.length)),
            if (ports.isNotEmpty && (checkingPort != null || checked.isNotEmpty)) ...[
              const SizedBox(height: 6),
              Semantics(liveRegion: true, child: Text(l.portCheckCompleted(checked.length, ports.length))),
              if (!busy) ...[
                const SizedBox(height: 6),
                LinearProgressIndicator(value: checked.length / ports.length, minHeight: 3),
              ],
            ],
            for (final number in ports.skip(offset).take(pageSize))
              resultCard(l, number, checked[number], listed: true),
            if (ports.length > pageSize)
              Row(
                children: [
                  IconButton(
                    tooltip: MaterialLocalizations.of(context).previousPageTooltip,
                    icon: const Icon(Icons.chevron_left),
                    onPressed: offset == 0 ? null : () => setState(() => offset -= pageSize),
                  ),
                  Text('${offset + 1}–${(offset + pageSize).clamp(0, ports.length)} / ${ports.length}'),
                  IconButton(
                    tooltip: MaterialLocalizations.of(context).nextPageTooltip,
                    icon: const Icon(Icons.chevron_right),
                    onPressed: offset + pageSize >= ports.length ? null : () => setState(() => offset += pageSize),
                  ),
                ],
              ),
          ],
          if (status != null && interval != null && !range) resultCard(l, interval.low, status, listed: false),
          if (error != null)
            Padding(
              padding: const EdgeInsets.only(top: 16),
              child: Semantics(
                liveRegion: true,
                child: Text(errorMessage(l, error!), style: GlassTokens.of(context).typography.bodyEmph),
              ),
            ),
        ],
      ),
    );
  }

  Widget resultCard(AppLocalizations l, int number, String? result, {required bool listed}) {
    final expanded = expandedPort == number;
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: PortResultCard(
        key: ValueKey('port-card-$number'),
        number: number,
        udp: udp,
        host: host.text.trim(),
        sourceId: remote ? source?.id.value : null,
        sourceLabel: remote ? source!.name : l.portCheckLocal,
        expanded: expanded,
        status: result,
        statusLabel: result == null ? null : _statusText(l, result),
        socket: resultTarget == null ? null : (hostId: resultTarget!.hostId, port: number, udp: udp),
        onToggle: busy
            ? null
            : () {
                setState(() => expandedPort = expanded ? null : number);
                if (!expanded && listed && result == null) checkListener(number);
              },
        onRetry: busy
            ? null
            : listed
            ? () => checkListener(number)
            : check,
      ),
    );
  }
}
