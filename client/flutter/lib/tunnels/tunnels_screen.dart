import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class TunnelsScreen extends ConsumerWidget {
  const TunnelsScreen({super.key});

  Future<void> _edit(BuildContext context, {Tunnel? tunnel}) =>
      showAppDialog<void>(context, builder: (_) => TunnelEditorDialog(tunnel: tunnel));

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tunnelsAsync = ref.watch(tunnelsProvider);
    final runtime = ref.watch(tunnelRuntimeProvider).value ?? const {};
    final hosts = ref.watch(hostByIdProvider);
    final l10n = context.l10n;
    return PageScaffold(
      title: l10n.tunnelsTitle,
      subtitle: l10n.tunnelsSubtitle,
      actions: [
        GlassButton(
          key: const ValueKey('add-tunnel'),
          onPressed: () => _edit(context),
          icon: Icons.add_rounded,
          label: l10n.tunnelsNew,
        ),
      ],
      body: AsyncValueView(
        value: tunnelsAsync,
        data: (tunnels) => tunnels.isEmpty
            ? EmptyState(
                icon: Icons.swap_horiz_rounded,
                title: l10n.tunnelsEmptyTitle,
                message: l10n.tunnelsEmptyMessage,
              )
            : ContentList(
                itemCount: tunnels.length,
                itemBuilder: (context, i) {
                  final t = tunnels[i];
                  final rt = runtime[t.id] ?? TunnelRuntime.stopped;
                  return _TunnelTile(
                    tunnel: t,
                    runtime: rt,
                    hostName: hosts[t.hostId]?.name ?? l10n.tunnelsDeletedHost,
                    onEdit: () => _edit(context, tunnel: t),
                  );
                },
              ),
      ),
    );
  }
}

class _TunnelTile extends ConsumerWidget {
  const _TunnelTile({required this.tunnel, required this.runtime, required this.hostName, required this.onEdit});

  final Tunnel tunnel;
  final TunnelRuntime runtime;
  final String hostName;
  final VoidCallback onEdit;

  /// `127.0.0.1:15432 → db.internal:5432` style summary (addresses are not
  /// translated; only the "remote" marker is).
  static String _summary(AppLocalizations l10n, Tunnel t) => switch (t.kind) {
    TunnelKind.local => '${t.bindHost}:${t.bindPort} → ${t.targetHost}:${t.targetPort}',
    TunnelKind.remote => l10n.tunnelsSummaryRemote('${t.bindHost}:${t.bindPort}', '${t.targetHost}:${t.targetPort}'),
    TunnelKind.dynamic => 'SOCKS5 ${t.bindHost}:${t.bindPort}',
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final t = tokens.typography;
    final l10n = context.l10n;
    final service = ref.read(tunnelServiceProvider);
    final since = runtime.since;
    final (color, label) = switch (runtime.state) {
      TunnelRunState.running => (
        p.success,
        since == null
            ? l10n.tunnelsStatusRunning(runtime.activeConnections)
            : l10n.tunnelsStatusRunningSince(runtime.activeConnections, formatRelative(l10n, since)),
      ),
      TunnelRunState.starting => (p.accent, l10n.tunnelsStatusStarting),
      // runtime.error is the core's English diagnostic (detail only).
      TunnelRunState.failed => (
        p.danger,
        runtime.error == null ? l10n.tunnelsStatusFailed : l10n.tunnelsStatusFailedDetail(runtime.error!),
      ),
      TunnelRunState.stopped => (p.tertiary, l10n.tunnelsStatusStopped),
    };
    final running = runtime.state == TunnelRunState.running || runtime.state == TunnelRunState.starting;
    return ListTile(
      key: ValueKey('tunnel-${tunnel.name}'),
      leading: Icon(switch (tunnel.kind) {
        TunnelKind.local => Icons.login_rounded,
        TunnelKind.remote => Icons.logout_rounded,
        TunnelKind.dynamic => Icons.hub_rounded,
      }),
      title: Row(
        children: [
          Flexible(
            child: Text(
              tunnel.name,
              overflow: TextOverflow.ellipsis,
              style: t.bodyEmph.copyWith(color: p.label),
            ),
          ),
          const SizedBox(width: GlassSpacing.s8),
          GlassBadge(label: tunnel.kind.localized(l10n), dense: true),
          if (tunnel.bindsPublicly) ...[
            const SizedBox(width: GlassSpacing.s6),
            Tooltip(
              message: l10n.tunnelsPublicBindTooltip(tunnel.bindHost),
              child: Icon(Icons.warning_rounded, color: p.danger, size: 18),
            ),
          ],
        ],
      ),
      subtitle: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            l10n.tunnelsTileSubtitle(_summary(l10n, tunnel), hostName),
            style: t.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
          ),
          const SizedBox(height: GlassSpacing.s2),
          Row(
            children: [
              StatusDot(color: color),
              const SizedBox(width: GlassSpacing.s6),
              Flexible(child: Text(label)),
            ],
          ),
        ],
      ),
      onTap: onEdit,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Switch.adaptive(
            key: ValueKey('tunnel-switch-${tunnel.name}'),
            value: running,
            onChanged: (v) => runWithFeedback(context, () => v ? service.start(tunnel.id) : service.stop(tunnel.id)),
          ),
          GlassIconButton(
            tooltip: l10n.commonDelete,
            icon: Icons.delete_rounded,
            style: GlassIconButtonStyle.plain,
            onPressed: () async {
              final ok = await showConfirmDialog(
                context,
                title: l10n.tunnelsDeleteTitle(tunnel.name),
                message: l10n.tunnelsDeleteMessage,
                confirmLabel: l10n.commonDelete,
                destructive: true,
              );
              if (ok && context.mounted) await runWithFeedback(context, () => service.deleteTunnel(tunnel.id));
            },
          ),
        ],
      ),
    );
  }
}

class TunnelEditorDialog extends ConsumerStatefulWidget {
  const TunnelEditorDialog({super.key, this.tunnel});

  final Tunnel? tunnel;

  @override
  ConsumerState<TunnelEditorDialog> createState() => _TunnelEditorDialogState();
}

class _TunnelEditorDialogState extends ConsumerState<TunnelEditorDialog> {
  late final TextEditingController _name = TextEditingController(text: widget.tunnel?.name ?? '');
  late final TextEditingController _bindHost = TextEditingController(text: widget.tunnel?.bindHost ?? '127.0.0.1');
  late final TextEditingController _bindPort = TextEditingController(text: widget.tunnel?.bindPort.toString() ?? '');
  late final TextEditingController _targetHost = TextEditingController(text: widget.tunnel?.targetHost ?? '');
  late final TextEditingController _targetPort = TextEditingController(
    text: widget.tunnel?.targetPort?.toString() ?? '',
  );
  late TunnelKind _kind = widget.tunnel?.kind ?? TunnelKind.local;
  late ObjectId? _hostId = widget.tunnel?.hostId;
  late bool _autoStart = widget.tunnel?.autoStart ?? false;
  bool _acknowledgedPublic = false;

  /// Rendered at build time so it follows the UI language.
  String Function(AppLocalizations l10n)? _error;

  @override
  void initState() {
    super.initState();
    _acknowledgedPublic = widget.tunnel?.bindsPublicly ?? false;
    _bindHost.addListener(() => setState(() {}));
  }

  @override
  void dispose() {
    for (final c in [_name, _bindHost, _bindPort, _targetHost, _targetPort]) {
      c.dispose();
    }
    super.dispose();
  }

  bool get _public => bindsPubliclyHost(_bindHost.text);

  Future<void> _save() async {
    final hostId = _hostId;
    if (hostId == null) {
      setState(() => _error = (l10n) => l10n.tunnelsErrorChooseHost);
      return;
    }
    final now = DateTime.now().toUtc();
    final tunnel = Tunnel(
      id: widget.tunnel?.id ?? ObjectId.generate(),
      name: _name.text.trim(),
      kind: _kind,
      hostId: hostId,
      bindHost: _bindHost.text.trim(),
      bindPort: int.tryParse(_bindPort.text) ?? 0,
      targetHost: _kind.needsTarget ? _targetHost.text.trim() : null,
      targetPort: _kind.needsTarget ? int.tryParse(_targetPort.text) : null,
      autoStart: _autoStart,
      createdAt: widget.tunnel?.createdAt ?? now,
      updatedAt: now,
    );
    final invalid = tunnel.validate();
    if (invalid != null) {
      setState(() => _error = (l10n) => errorMessage(l10n, invalid));
      return;
    }
    try {
      await ref.read(tunnelServiceProvider).saveTunnel(tunnel);
      if (mounted) closeDialog<void>(context);
    } on AppException catch (e) {
      setState(() => _error = (l10n) => errorMessage(l10n, e));
    }
  }

  @override
  Widget build(BuildContext context) {
    final hosts = (ref.watch(hostsProvider).value ?? const <Host>[]).where((h) => !h.isRdp).toList()
      ..sort((a, b) => a.name.compareTo(b.name));
    final digits = [FilteringTextInputFormatter.digitsOnly];
    final remote = _kind == TunnelKind.remote;
    final l10n = context.l10n;
    final error = _error;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      key: const ValueKey('tunnel-editor'),
      title: widget.tunnel == null ? l10n.tunnelsNew : l10n.tunnelsEdit,
      width: 588,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GlassSegmented<TunnelKind>(
              key: const ValueKey('tunnel-kind'),
              inChrome: false,
              expand: true,
              segments: [for (final k in TunnelKind.values) GlassSegment(value: k, label: k.localized(l10n))],
              selected: _kind,
              onChanged: (k) => setState(() => _kind = k),
            ),
            const SizedBox(height: GlassSpacing.s4),
            Text(
              _kind.localizedDescription(l10n),
              style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              key: const ValueKey('tunnel-name'),
              controller: _name,
              decoration: InputDecoration(labelText: l10n.commonName),
            ),
            const SizedBox(height: GlassSpacing.s12),
            DropdownButtonFormField<ObjectId>(
              isExpanded: true,
              borderRadius: BorderRadius.circular(tokens.radii.menu),
              key: const ValueKey('tunnel-host'),
              initialValue: hosts.any((h) => h.id == _hostId) ? _hostId : null,
              decoration: InputDecoration(labelText: l10n.tunnelsHostLabel),
              items: [for (final h in hosts) DropdownMenuItem(value: h.id, child: Text('${h.name} (${h.address})'))],
              onChanged: (v) => setState(() => _hostId = v),
            ),
            const SizedBox(height: GlassSpacing.s12),
            Row(
              children: [
                Expanded(
                  flex: 2,
                  child: TextField(
                    key: const ValueKey('tunnel-bind-host'),
                    controller: _bindHost,
                    decoration: InputDecoration(
                      labelText: remote ? l10n.tunnelsBindAddressRemote : l10n.tunnelsBindAddressLocal,
                    ),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: TextField(
                    key: const ValueKey('tunnel-bind-port'),
                    controller: _bindPort,
                    inputFormatters: digits,
                    decoration: InputDecoration(labelText: l10n.tunnelsPortLabel),
                  ),
                ),
              ],
            ),
            if (_kind.needsTarget) ...[
              const SizedBox(height: GlassSpacing.s12),
              Row(
                children: [
                  Expanded(
                    flex: 2,
                    child: TextField(
                      key: const ValueKey('tunnel-target-host'),
                      controller: _targetHost,
                      decoration: InputDecoration(
                        labelText: remote ? l10n.tunnelsTargetRemote : l10n.tunnelsTargetLocal,
                      ),
                    ),
                  ),
                  const SizedBox(width: GlassSpacing.s12),
                  Expanded(
                    child: TextField(
                      key: const ValueKey('tunnel-target-port'),
                      controller: _targetPort,
                      inputFormatters: digits,
                      decoration: InputDecoration(labelText: l10n.tunnelsPortLabel),
                    ),
                  ),
                ],
              ),
            ],
            if (_public) ...[
              const SizedBox(height: GlassSpacing.s12),
              InfoBanner(
                key: const ValueKey('public-bind-warning'),
                tone: BannerTone.danger,
                title: l10n.tunnelsPublicBindTitle,
                message: remote
                    ? l10n.tunnelsPublicBindWarningRemote(_bindHost.text)
                    : l10n.tunnelsPublicBindWarningLocal(_bindHost.text),
              ),
              CheckboxListTile(
                key: const ValueKey('ack-public-bind'),
                contentPadding: EdgeInsets.zero,
                value: _acknowledgedPublic,
                onChanged: (v) => setState(() => _acknowledgedPublic = v ?? false),
                title: Text(l10n.tunnelsPublicBindAck),
              ),
            ],
            SwitchListTile.adaptive(
              contentPadding: EdgeInsets.zero,
              value: _autoStart,
              onChanged: (v) => setState(() => _autoStart = v),
              title: Text(l10n.tunnelsAutoStart),
            ),
            if (error != null) GateErrorText(text: error(l10n)),
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('save-tunnel'),
        onPressed: _public && !_acknowledgedPublic ? null : _save,
        label: l10n.commonSave,
      ),
    );
  }
}
