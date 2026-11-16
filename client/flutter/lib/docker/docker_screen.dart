import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_workspace.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_receiver.dart';
import 'package:consolecrypt/monitoring/job_context_route_guard.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Docker section: pick this computer or a saved SSH host, then browse its
/// containers. The screen owns the engine session and closes it when the
/// section is left for good (lock, profile switch).
class DockerScreen extends ConsumerStatefulWidget {
  const DockerScreen({super.key, this.contextFocus});
  final JobContextFocus? contextFocus;
  @override
  ConsumerState<DockerScreen> createState() => _DockerScreenState();
}

class _DockerScreenState extends ConsumerState<DockerScreen> {
  DockerSessionInfo? _session;
  DockerHost? _host;
  DockerService? _service;
  JobContextFocus? _handledFocus, _acceptedFocus;
  int _focusGeneration = 0;
  @override
  void initState() {
    super.initState();
    _scheduleFocus();
  }
  @override
  void didUpdateWidget(DockerScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.contextFocus, widget.contextFocus)) {
      _focusGeneration++;
      _scheduleFocus();
    }
  }
  void _scheduleFocus() => WidgetsBinding.instance.addPostFrameCallback((_) {
    if (mounted) unawaited(_consumeFocus());
  });
  bool _focusCurrent(JobContextFocus focus, int token) => mounted && token == _focusGeneration &&
    identical(widget.contextFocus, focus) &&
    ref.read(routerProvider).routeInformationProvider.value.uri.path == AppRoutes.docker &&
    ref.read(monitoringScopeProvider).current && identical(ref.read(monitoringSessionProvider)?.correlationScope, focus.scope);
  Future<bool> _recheckFocus(JobContextFocus focus) async {
    final token = _focusGeneration, original = ref.read(monitoringSessionProvider);
    final service = original?.service;
    if (!identical(_acceptedFocus, focus) || !_focusCurrent(focus, token) ||
        service is! MonitoringHostContextReader) {
      return false;
    }
    final reader = service as MonitoringHostContextReader;
    if (!reader.hostContextSupported) return false;
    try {
      final checked = await reader.recheckHostContext(focus.metadata);
      return _focusCurrent(focus, token) && identical(ref.read(monitoringSessionProvider), original) &&
        identical(_acceptedFocus, focus) && sameJobContextMetadata(focus.metadata, checked);
    } catch (_) {
      return false;
    }
  }
  Future<void> _consumeFocus() async {
    final focus = widget.contextFocus;
    if (focus == null || identical(focus, _handledFocus)) return;
    _handledFocus = focus;
    final token = ++_focusGeneration, previous = _session;
    final originalService = ref.read(dockerServiceProvider);
    if (originalService == null) return;
    bool originalCurrent() => _focusCurrent(focus, token) && identical(ref.read(dockerServiceProvider), originalService);
    try {
      final ready = await receiveJobContextFocus(ref, focus, destination: JobContextDestination.dockerHost,
        receiverCurrent: () => originalCurrent() && identical(_session, previous) &&
          ModalRoute.of(context)?.isCurrent == true);
      if (!mounted || ready == null || !originalCurrent() || !identical(_session, previous)) return;
      setState(() => _acceptedFocus = ready);
      if (previous == null) return;
      // The real A chooser is read-only while B remains connected. No new
      // switch/approval path: the user closes this chooser and uses the existing
      // explicit Disconnect B action before the ordinary Connect A action.
      await showAppDialog<void>(context, builder: (dialogContext) => JobContextRouteGuard(focus: ready,
        isCurrent: originalCurrent, child: GlassDialog(
        title: context.l10n.dockerTitle, width: 720,
        content: SizedBox(height: 420, child: Column(children: [
          InfoBanner(message: context.l10n.correlationDisconnectBeforeConnect, tone: BannerTone.info),
          Expanded(child: DockerHostPicker(service: originalService, contextFocus: ready, connectEnabled: false,
            onConnected: (_, _) {})),
        ])),
        secondaryActions: [GlassButton(label: context.l10n.commonClose,
          onPressed: () => Navigator.of(dialogContext).pop())])));
    } catch (_) {
      if (mounted && _focusCurrent(focus, token)) showSnack(context, context.l10n.correlationContextChanged, error: true);
    }
  }

  @override
  void dispose() {
    _focusGeneration++;
    _acceptedFocus = null;
    _disconnect();
    super.dispose();
  }

  void _disconnect() {
    final session = _session, service = _service;
    _session = null;
    if (session != null && service != null) unawaited(service.disconnect(session.id).catchError((Object _) {}));
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dockerScopeProvider, (previous, next) {
      if (previous?.profileId != next.profileId || !next.unlocked) {
        setState(() {
        _focusGeneration++; _acceptedFocus = null; _disconnect();
      });
      }
    });
    ref.listen(monitoringSessionProvider, (previous, next) {
      if (!identical(previous, next)) setState(() { _focusGeneration++; _acceptedFocus = null; });
    });
    final service = ref.watch(dockerServiceProvider);
    final l = context.l10n;
    if (service == null) {
      return PageScaffold(
        title: l.dockerTitle,
        subtitle: l.dockerSubtitle,
        body: EmptyState(
          key: const ValueKey('docker-unavailable'),
          icon: Icons.view_in_ar_outlined,
          title: l.dockerUnavailable,
          message: l.dockerUnavailableHelp,
        ),
      );
    }
    final session = _session;
    if (session != null && identical(service, _service)) {
      return DockerWorkspace(
        key: ValueKey(session.id),
        service: service,
        session: session,
        host: _host,
        onDisconnect: () => setState(_disconnect),
      );
    }
    final acceptedFocus = _acceptedFocus, focusEpoch = _focusGeneration;
    return DockerHostPicker(
      service: service,
      contextFocus: acceptedFocus,
      beforeContextConnect: acceptedFocus == null ? null : () => _recheckFocus(acceptedFocus),
      contextCurrent: acceptedFocus == null ? null : () => identical(_acceptedFocus, acceptedFocus) &&
        _focusCurrent(acceptedFocus, focusEpoch),
      onConnected: (host, info) => setState(() {
        _disconnect();
        _service = service;
        _host = host;
        _session = info;
      }),
    );
  }
}

/// "This computer" and saved SSH hosts, with search. Connecting shows
/// progress; a failure explains what to do on the host.
class DockerHostPicker extends ConsumerStatefulWidget {
  const DockerHostPicker({required this.service, required this.onConnected, super.key,
    this.contextFocus, this.connectEnabled = true, this.beforeContextConnect, this.contextCurrent});
  final JobContextFocus? contextFocus;
  final bool connectEnabled;
  final Future<bool> Function()? beforeContextConnect;
  final bool Function()? contextCurrent;
  final DockerService service;
  final void Function(DockerHost host, DockerSessionInfo session) onConnected;
  @override
  ConsumerState<DockerHostPicker> createState() => _DockerHostPickerState();
}

class _DockerHostPickerState extends ConsumerState<DockerHostPicker> {
  late Future<List<DockerHost>> _hosts = widget.service.hosts();
  String _query = '';
  String? _connecting;
  DockerHost? _failedHost;
  Object? _error;
  int _generation = 0;
  bool _contextInvalid = false;
  @override
  void didUpdateWidget(DockerHostPicker oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.contextFocus, oldWidget.contextFocus) || !identical(widget.service, oldWidget.service)) {
      _generation++;
      _connecting = null;
      _contextInvalid = false;
      _hosts = widget.service.hosts();
    }
  }

  void _reload() => setState(() {
    _hosts = widget.service.hosts();
    _error = null;
    _failedHost = null;
  });

  Future<void> _connect(DockerHost host) async {
    if (_connecting != null || !widget.connectEnabled) return;
    final focus = widget.contextFocus;
    final originalService = widget.service;
    if (focus != null && (host.id != focus.selection.hostId || widget.beforeContextConnect == null ||
        widget.contextCurrent?.call() != true)) {
      return;
    }
    final generation = ++_generation;
    setState(() {
      _connecting = host.id;
      _error = null;
      _failedHost = null;
    });
    try {
      if (focus != null) {
        final accepted = await widget.beforeContextConnect!();
        if (!accepted || !mounted || generation != _generation || !identical(widget.contextFocus, focus) ||
            !identical(widget.service, originalService) || !widget.connectEnabled ||
            widget.contextCurrent?.call() != true) {
          if (mounted && generation == _generation) {
            setState(() {
            _connecting = null; _contextInvalid = true;
          });
          }
          return;
        }
      }
      final session = await originalService.connect(host.id);
      if (!mounted || generation != _generation ||
          focus != null && (!identical(widget.contextFocus, focus) || widget.contextCurrent?.call() != true ||
            session.hostId != focus.selection.hostId)) {
        unawaited(originalService.disconnect(session.id).catchError((Object _) {}));
        return;
      }
      setState(() => _connecting = null);
      widget.onConnected(host, session);
    } catch (error) {
      if (!mounted || generation != _generation) return;
      setState(() {
        _connecting = null;
        _error = error;
        _failedHost = host;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final focus = widget.contextFocus;
    final originalSession = focus == null ? null : ref.watch(monitoringSessionProvider);
    final scopeCurrent = focus == null || identical(originalSession?.correlationScope, focus.scope);
    final l = context.l10n;
    final narrow = MediaQuery.sizeOf(context).width < 600;
    return PageScaffold(
      title: l.dockerTitle,
      subtitle: l.dockerSubtitle,
      actions: [
        GlassIconButton(
          key: const ValueKey('docker-hosts-refresh'),
          icon: Icons.refresh,
          tooltip: l.dockerRefresh,
          onPressed: _connecting == null ? _reload : null,
        ),
      ],
      body: FutureBuilder<List<DockerHost>>(
        future: _hosts,
        builder: (context, snapshot) {
          final needle = _query.trim().toLowerCase();
          final hosts = (snapshot.data ?? const <DockerHost>[])
              .where((h) => (focus == null || scopeCurrent && h.kind == DockerHostKind.ssh && h.id == focus.selection.hostId) &&
                (needle.isEmpty || '${h.name} ${h.address}'.toLowerCase().contains(needle)))
              .toList();
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (focus != null) InfoBanner(message: _contextInvalid || !scopeCurrent ?
                l.correlationContextChanged : l.correlationFocusedHostHint,
                tone: _contextInvalid || !scopeCurrent ? BannerTone.danger : BannerTone.info),
              Align(
                alignment: Alignment.centerLeft,
                child: SizedBox(
                  width: narrow ? double.infinity : 320,
                  child: GlassField(
                    key: const ValueKey('docker-host-search'),
                    fieldKey: const ValueKey('docker-host-search-input'),
                    search: true,
                    placeholder: l.dockerSearchHosts,
                    onChanged: (value) => setState(() => _query = value),
                  ),
                ),
              ),
              const SizedBox(height: GlassSpacing.s12),
              if (_error != null && _failedHost != null) ...[
                DockerConnectError(
                  host: _failedHost!,
                  error: _error!,
                  onRetry: _connecting == null ? () => _connect(_failedHost!) : null,
                ),
                const SizedBox(height: GlassSpacing.s12),
              ],
              if (snapshot.connectionState != ConnectionState.done) const LinearProgressIndicator(minHeight: 2),
              if (snapshot.hasError)
                InfoBanner(tone: BannerTone.danger, message: l.dockerHostsFailed)
              else if (snapshot.connectionState == ConnectionState.done && hosts.isEmpty)
                Expanded(
                  child: EmptyState(icon: Icons.dns_outlined, title: l.dockerNoHosts, message: l.dockerNoHostsHelp),
                )
              else
                Expanded(
                  child: ContentList(
                    key: const ValueKey('docker-hosts'),
                    itemCount: hosts.length,
                    itemBuilder: (context, index) => _HostRow(
                      host: hosts[index],
                      connecting: _connecting == hosts[index].id,
                      selected: focus != null,
                      enabled: _connecting == null && widget.connectEnabled && scopeCurrent && !_contextInvalid &&
                        (focus == null || widget.beforeContextConnect != null),
                      onConnect: () => _connect(hosts[index]),
                    ),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }
}

class _HostRow extends StatelessWidget {
  const _HostRow({required this.host, required this.connecting, required this.enabled, required this.onConnect, this.selected = false});
  final bool selected;
  final DockerHost host;
  final bool connecting, enabled;
  final VoidCallback onConnect;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final local = host.kind == DockerHostKind.local;
    final narrow = MediaQuery.sizeOf(context).width < 600;
    final subtitle = local ? l.dockerLocalHostHelp : host.address;
    return GlassInteractive(
      key: ValueKey('docker-host-${host.id}'),
      semanticLabel: host.name,
      onPressed: enabled ? onConnect : null,
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: selected || state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          child: Row(
            children: [
              Icon(local ? Icons.laptop_mac_rounded : Icons.dns_outlined, size: 24, color: tokens.palette.accent),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      local ? l.dockerLocalHost : host.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                    ),
                    if (subtitle.isNotEmpty) ...[
                      const SizedBox(height: 2),
                      Text(
                        subtitle,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: (local ? tokens.typography.caption : tokens.typography.mono.copyWith(fontSize: 12))
                            .copyWith(color: tokens.secondaryLabel),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 8),
              if (narrow)
                GlassIconButton(
                  key: ValueKey('docker-connect-${host.id}'),
                  icon: Icons.play_arrow_rounded,
                  tooltip: l.dockerConnect,
                  onPressed: enabled ? onConnect : null,
                )
              else
                GlassButton.prominent(
                  key: ValueKey('docker-connect-${host.id}'),
                  label: l.dockerConnect,
                  icon: Icons.play_arrow_rounded,
                  busy: connecting,
                  busyLabel: l.dockerConnecting,
                  onPressed: enabled ? onConnect : null,
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// What went wrong when connecting and what to do about it. The group fix is
/// shown as a command to copy; the app never runs it.
class DockerConnectError extends ConsumerWidget {
  const DockerConnectError({required this.host, required this.error, this.onRetry, super.key});
  final DockerHost host;
  final Object error;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final name = host.kind == DockerHostKind.local ? l.dockerLocalHost : host.name;
    final failure = error is DockerException ? (error as DockerException).failure : DockerFailure.other;
    final detail = error is DockerException ? (error as DockerException).detail.trim() : '';
    final retry = GlassButton(
      key: const ValueKey('docker-connect-retry'),
      label: l.dockerReconnect,
      icon: Icons.refresh,
      onPressed: onRetry,
    );
    switch (failure) {
      case DockerFailure.permissionDenied:
        final group = detail.isEmpty ? 'docker' : detail; // l10n-ignore: Unix group name
        final user = host.kind == DockerHostKind.ssh ? ref.watch(hostByIdProvider)[ObjectId(host.id)]?.username : null;
        final command =
            'sudo usermod -aG $group ${user == null || user.isEmpty ? r'$USER' : user}'; // l10n-ignore: shell command
        return InfoBanner(
          key: const ValueKey('docker-error-permission'),
          tone: BannerTone.warning,
          icon: Icons.lock_outline_rounded,
          title: l.dockerPermissionDeniedTitle,
          message: l.dockerPermissionDenied(name, group),
          action: Padding(
            padding: const EdgeInsets.only(top: GlassSpacing.s8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                ContentSurface(
                  kind: ContentSurfaceKind.solid,
                  padding: const EdgeInsets.fromLTRB(12, 6, 4, 6),
                  child: Row(
                    children: [
                      Expanded(
                        child: SelectableText(
                          command,
                          key: const ValueKey('docker-permission-command'),
                          style: tokens.typography.mono.copyWith(color: tokens.palette.label),
                        ),
                      ),
                      GlassIconButton(
                        key: const ValueKey('docker-permission-copy'),
                        icon: Icons.content_copy_rounded,
                        tooltip: l.dockerCopyCommand,
                        style: GlassIconButtonStyle.plain,
                        onPressed: () => copyPlainWithNotice(context, ref, command, what: l.copyWhatCommand),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: GlassSpacing.s6),
                Text(
                  l.dockerPermissionDeniedNote,
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
                const SizedBox(height: GlassSpacing.s8),
                Align(alignment: AlignmentDirectional.centerEnd, child: retry),
              ],
            ),
          ),
        );
      case DockerFailure.notRunning:
        return InfoBanner(
          key: const ValueKey('docker-error-not-running'),
          tone: BannerTone.warning,
          title: l.dockerNotRunningTitle,
          message: l.dockerNotRunning(name),
          action: retry,
        );
      case DockerFailure.connectionFailed:
      case DockerFailure.other:
        return InfoBanner(
          key: const ValueKey('docker-error-connection'),
          tone: BannerTone.danger,
          title: failure == DockerFailure.connectionFailed ? l.dockerConnectionFailedTitle : l.dockerRequestFailed,
          message: [
            l.dockerConnectionFailed(name),
            if (detail == 'socket_forwarding_disabled')
              l
                  .dockerSocketForwardingDisabled // l10n-ignore: wire code
            else if (detail.isNotEmpty)
              detail,
          ].join('\n'),
          action: retry,
        );
    }
  }
}
