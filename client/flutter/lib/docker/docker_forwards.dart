import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Asks for the container port (prefilled with its exposed ports) and the
/// local port (0 = any free port), then starts the forward. Returns true
/// when a forward was created.
Future<bool?> showDockerForwardDialog(
  BuildContext context, {
  required DockerService service,
  required DockerSessionInfo session,
  required DockerContainer container,
}) => showAppDialog<bool>(
  context,
  secure: true,
  builder: (_) => _ForwardDialog(service: service, session: session, container: container),
);

class _ForwardDialog extends ConsumerStatefulWidget {
  const _ForwardDialog({required this.service, required this.session, required this.container});
  final DockerService service;
  final DockerSessionInfo session;
  final DockerContainer container;
  @override
  ConsumerState<_ForwardDialog> createState() => _ForwardDialogState();
}

class _ForwardDialogState extends ConsumerState<_ForwardDialog> {
  final _remote = TextEditingController(), _local = TextEditingController(text: '0');
  List<int> _ports = const [];
  bool _busy = false;
  Object? _error;
  DockerForward? _created;

  @override
  void initState() {
    super.initState();
    _ports = dockerForwardablePorts(widget.container);
    if (_ports.isNotEmpty) _remote.text = '${_ports.first}';
    unawaited(_loadExposed());
  }

  /// `EXPOSE`d ports without a binding appear only in inspect.
  Future<void> _loadExposed() async {
    try {
      final detail = await widget.service.inspect(widget.session.id, widget.container.id);
      if (!mounted) return;
      final ports = dockerForwardablePorts(widget.container, detail.inspect);
      setState(() {
        _ports = ports;
        if (_remote.text.isEmpty && ports.isNotEmpty) _remote.text = '${ports.first}';
      });
    } catch (_) {
      // The published ports are enough to offer.
    }
  }

  @override
  void dispose() {
    _remote.dispose();
    _local.dispose();
    super.dispose();
  }

  int? get _remotePort {
    final value = int.tryParse(_remote.text);
    return value != null && value > 0 && value <= 65535 ? value : null;
  }

  int? get _localPort {
    final value = int.tryParse(_local.text.isEmpty ? '0' : _local.text);
    return value != null && value >= 0 && value <= 65535 ? value : null;
  }

  Future<void> _start() async {
    final remote = _remotePort, local = _localPort;
    if (_busy || remote == null || local == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final forward = await widget.service.startForward(
        widget.session.id,
        widget.container.id,
        remotePort: remote,
        localPort: local,
      );
      if (!mounted) {
        unawaited(widget.service.stopForward(forward.id).catchError((Object _) {}));
        return;
      }
      ref.invalidate(dockerForwardsProvider);
      setState(() => _created = forward);
    } catch (error) {
      if (mounted) setState(() => _error = error);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final created = _created;
    final valid = _remotePort != null && _localPort != null;
    final local = widget.session.hostId == DockerHost.localId;
    return GlassDialog(
      key: const ValueKey('docker-forward-dialog'),
      title: l.dockerForwardPort,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.container.name, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
            const SizedBox(height: 4),
            Text(
              local ? l.dockerForwardHelpLocal : l.dockerForwardHelpSsh,
              style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
            ),
            const SizedBox(height: GlassSpacing.s12),
            if (created != null) ...[
              Row(
                children: [
                  Expanded(
                    child: SelectableText(
                      dockerForwardLocalAddress(created.address),
                      key: const ValueKey('docker-forward-address'),
                      style: tokens.typography.mono.copyWith(color: tokens.palette.label, fontWeight: FontWeight.w600),
                    ),
                  ),
                  GlassIconButton(
                    icon: Icons.content_copy_rounded,
                    tooltip: l.dockerForwardCopyAddress,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () =>
                        unawaited(Clipboard.setData(ClipboardData(text: dockerForwardLocalAddress(created.address)))),
                  ),
                ],
              ),
              Text(l.dockerForwardCreated, style: tokens.typography.callout.copyWith(color: tokens.palette.success)),
            ] else ...[
              TextField(
                key: const ValueKey('docker-forward-remote'),
                controller: _remote,
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(5)],
                enabled: !_busy,
                decoration: InputDecoration(labelText: l.dockerForwardContainerPort),
                onChanged: (_) => setState(() {}),
              ),
              if (_ports.isNotEmpty) ...[
                const SizedBox(height: 6),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final port in _ports)
                      ChoiceChip(
                        key: ValueKey('docker-forward-port-$port'),
                        label: Text('$port'),
                        selected: _remote.text == '$port',
                        onSelected: _busy ? null : (_) => setState(() => _remote.text = '$port'),
                      ),
                  ],
                ),
              ],
              const SizedBox(height: GlassSpacing.s8),
              TextField(
                key: const ValueKey('docker-forward-local'),
                controller: _local,
                keyboardType: TextInputType.number,
                inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(5)],
                enabled: !_busy,
                decoration: InputDecoration(labelText: l.dockerForwardLocalPort, helperText: l.dockerForwardLocalHelp),
                onChanged: (_) => setState(() {}),
              ),
              if (_error != null) ...[
                const SizedBox(height: GlassSpacing.s8),
                Text(
                  dockerErrorText(l, _error!),
                  key: const ValueKey('docker-forward-error'),
                  style: tokens.typography.callout.copyWith(color: tokens.palette.danger),
                ),
              ],
              if (_busy) const Padding(padding: EdgeInsets.only(top: 8), child: LinearProgressIndicator(minHeight: 2)),
            ],
          ],
        ),
      ),
      primaryAction: created != null
          ? GlassButton.prominent(
              key: const ValueKey('docker-forward-done'),
              label: l.commonClose,
              onPressed: () => closeDialog(context, true),
            )
          : GlassButton.prominent(
              key: const ValueKey('docker-forward-start'),
              label: l.dockerForwardStart,
              busy: _busy,
              onPressed: valid && !_busy ? _start : null,
            ),
      secondaryActions: created != null
          ? const []
          : [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
      onSubmit: created == null && valid && !_busy ? _start : null,
    );
  }
}

/// `localhost:PORT` for a wildcard bind, so the copied address works.
String dockerForwardLocalAddress(String address) =>
    address.startsWith('0.0.0.0:') ? 'localhost:${address.substring(8)}' : address; // l10n-ignore: host name

/// Running Docker forwards: copyable address, traffic and a stop button.
/// Refreshes every 3 s while visible and in the foreground.
class DockerForwardsPanel extends ConsumerStatefulWidget {
  const DockerForwardsPanel({required this.service, this.onClose, this.embedded = false, super.key});
  final DockerService service;
  final VoidCallback? onClose;

  /// Inside the shared forwards panel: the cards only (nothing when empty).
  final bool embedded;
  @override
  ConsumerState<DockerForwardsPanel> createState() => _DockerForwardsPanelState();
}

class _DockerForwardsPanelState extends ConsumerState<DockerForwardsPanel> with WidgetsBindingObserver {
  Timer? _timer;
  bool _foreground = true;
  final _stopping = <String>{};

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _timer = Timer.periodic(const Duration(seconds: 3), (_) {
      if (mounted &&
          _foreground &&
          TickerMode.valuesOf(context).enabled &&
          !ref.read(dockerForwardsProvider).isLoading) {
        ref.invalidate(dockerForwardsProvider);
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) => _foreground = state == AppLifecycleState.resumed;

  Future<void> _stop(DockerForward forward) async {
    if (_stopping.contains(forward.id)) return;
    setState(() => _stopping.add(forward.id));
    try {
      await widget.service.stopForward(forward.id);
      if (mounted) ref.invalidate(dockerForwardsProvider);
    } catch (error) {
      if (mounted) showSnack(context, dockerErrorText(context.l10n, error), error: true);
    } finally {
      if (mounted) setState(() => _stopping.remove(forward.id));
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final result = ref.watch(dockerForwardsProvider);
    final forwards = result.value ?? const <DockerForward>[];
    if (widget.embedded) {
      return Column(
        key: const ValueKey('docker-forwards-embedded'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final forward in forwards) ...[_card(context, forward), const SizedBox(height: GlassSpacing.s8)],
        ],
      );
    }
    return ContentSurface(
      key: const ValueKey('docker-forwards-panel'),
      kind: ContentSurfaceKind.solid,
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s8),
            child: Row(
              children: [
                Expanded(
                  child: Text(l.dockerForwards, style: tokens.typography.title3.copyWith(color: tokens.palette.label)),
                ),
                if (widget.onClose != null)
                  GlassIconButton(
                    key: const ValueKey('docker-forwards-close'),
                    icon: Icons.close_rounded,
                    tooltip: l.commonClose,
                    style: GlassIconButtonStyle.plain,
                    onPressed: widget.onClose,
                  ),
              ],
            ),
          ),
          if (result.hasError)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, 0, GlassSpacing.s16, GlassSpacing.s8),
              child: Text(
                dockerErrorText(l, result.error!),
                style: tokens.typography.callout.copyWith(color: tokens.palette.danger),
              ),
            ),
          Expanded(
            child: forwards.isEmpty
                ? EmptyState(
                    icon: Icons.swap_horiz_rounded,
                    title: l.dockerNoForwards,
                    message: l.dockerNoForwardsHelp,
                    compact: true,
                  )
                : ListView.separated(
                    padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, 0, GlassSpacing.s12, GlassSpacing.s12),
                    itemCount: forwards.length,
                    separatorBuilder: (_, _) => const SizedBox(height: GlassSpacing.s8),
                    itemBuilder: (context, index) => _card(context, forwards[index]),
                  ),
          ),
        ],
      ),
    );
  }

  Widget _card(BuildContext context, DockerForward forward) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final running = forward.state == DockerForwardState.running;
    final address = dockerForwardLocalAddress(forward.address);
    final state = switch (forward.state) {
      DockerForwardState.running => l.dockerForwardRunning,
      DockerForwardState.failed => l.dockerForwardFailed,
      DockerForwardState.stopped => l.dockerForwardStopped,
    };
    return ContentSurface(
      key: ValueKey('docker-forward-${forward.id}'),
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s4, GlassSpacing.s8),
      child: Row(
        children: [
          StatusDot(
            color: running
                ? p.success
                : forward.state == DockerForwardState.failed
                ? p.danger
                : p.tertiary,
          ),
          const SizedBox(width: GlassSpacing.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  address,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.mono.copyWith(color: p.label, fontWeight: FontWeight.w600),
                ),
                const SizedBox(height: 2),
                Text(
                  '→ ${forward.containerName}:${forward.remotePort}',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.callout.copyWith(color: p.label),
                ),
                const SizedBox(height: 2),
                Text(
                  [
                    state,
                    l.dockerForwardTraffic(
                      forward.activeConnections,
                      formatBytes(l, forward.bytesSent),
                      formatBytes(l, forward.bytesReceived),
                    ),
                    ?forward.errorCode,
                  ].join(' · '),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
              ],
            ),
          ),
          GlassIconButton(
            key: ValueKey('docker-forward-copy-${forward.id}'),
            icon: Icons.content_copy_rounded,
            tooltip: l.dockerForwardCopyAddress,
            style: GlassIconButtonStyle.plain,
            onPressed: () => unawaited(Clipboard.setData(ClipboardData(text: address))),
          ),
          GlassIconButton(
            key: ValueKey('docker-forward-stop-${forward.id}'),
            icon: Icons.stop_circle_outlined,
            tooltip: l.dockerForwardStop,
            color: p.danger,
            style: GlassIconButtonStyle.plain,
            onPressed: _stopping.contains(forward.id) ? null : () => _stop(forward),
          ),
        ],
      ),
    );
  }
}
