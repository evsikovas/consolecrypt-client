import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/hosts/host_metrics_view.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final hostMetricsReaderProvider = Provider<HostMetricsReader?>(
  (ref) => ref.watch(appServicesProvider.select((app) => app.hostMetrics)),
);

class HostMetricsScreen extends ConsumerStatefulWidget {
  const HostMetricsScreen({super.key, required this.hostId});

  final ObjectId hostId;

  @override
  ConsumerState<HostMetricsScreen> createState() => _HostMetricsScreenState();
}

class _HostMetricsScreenState extends ConsumerState<HostMetricsScreen> {
  late final _app = ref.read(appServicesProvider);
  late final _profile = ref.read(criticalNoticeScopeProvider);
  late final _events = ref.read(criticalRuntimeEventsProvider);
  late final _reader = ref.read(hostMetricsReaderProvider);
  StreamSubscription<RuntimeNoticeFrame>? _subscription;
  RuntimeNoticeFrame? _origin;
  Host? _host;
  bool _ended = false;

  bool get _profileCurrent =>
      mounted &&
      _profile.unlocked &&
      identical(ref.read(appServicesProvider), _app) &&
      identical(ref.read(criticalNoticeScopeProvider), _profile);

  bool _accepts(RuntimeNoticeFrame? frame) =>
      frame != null && !frame.streamUnavailable && frame.scope != null && frame.scope!.profileId == _profile.profileId;

  bool get _watchCurrent {
    final origin = _origin, frame = _events?.currentScope;
    return !_ended &&
        _profileCurrent &&
        _reader != null &&
        identical(ref.read(hostMetricsReaderProvider), _reader) &&
        origin != null &&
        _accepts(frame) &&
        origin.epoch == frame!.epoch &&
        origin.scope!.same(frame.scope);
  }

  @override
  void initState() {
    super.initState();
    final frame = _events?.currentScope;
    if (_accepts(frame)) _origin = frame;
    _subscription = _events?.watchRuntimeNotices().listen(
      (_) {
        if (!mounted || _ended) return;
        final current = _events.currentScope;
        if (_origin == null && _profileCurrent && _accepts(current)) {
          setState(() => _origin = current);
        } else if (current?.streamUnavailable == true || (_origin != null && !_watchCurrent)) {
          _retire();
        }
      },
      onError: (Object _) => _retire(),
      onDone: _retire,
    );
  }

  void _retire() {
    if (mounted && !_ended) setState(() => _ended = true);
  }

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    super.dispose();
  }

  bool _sameHost(Host? saved, Host original) =>
      saved != null && !saved.isRdp && saved.updatedAt.isAtSameMomentAs(original.updatedAt);

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final hosts = ref.watch(hostsProvider);
    final saved = hosts.value?.where((host) => host.id == widget.hostId).firstOrNull;
    ref.watch(appServicesProvider);
    ref.watch(criticalNoticeScopeProvider);
    ref.watch(criticalRuntimeEventsProvider);
    ref.watch(hostMetricsReaderProvider);
    final router = ref.watch(routerProvider);
    final path = AppRoutes.hostMetrics(widget.hostId);
    _host ??= saved;
    final host = _host;

    bool current() =>
        host != null &&
        _watchCurrent &&
        router.routeInformationProvider.value.uri.path == path &&
        _sameHost(ref.read(hostsProvider).value?.where((candidate) => candidate.id == widget.hostId).firstOrNull, host);

    return ValueListenableBuilder(
      valueListenable: router.routeInformationProvider,
      builder: (context, route, _) => Column(
        key: const ValueKey('host-metrics-page'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
            child: Row(
              children: [
                GlassIconButton(
                  key: const ValueKey('host-metrics-back'),
                  icon: Icons.arrow_back_rounded,
                  tooltip: l.commonBack,
                  onPressed: () => router.go(AppRoutes.hosts),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    host == null ? l.hostMetricsTitle : '${l.hostMetricsTitle} · ${host.name}',
                    style: Theme.of(context).textTheme.headlineSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
          Expanded(
            child: host != null && route.uri.path == path && current()
                ? HostMetricsView(host: host, reader: _reader, scope: _origin!.scope, active: true, isCurrent: current)
                : Center(
                    child:
                        !hosts.hasValue && hosts.isLoading ||
                            _origin == null &&
                                !_ended &&
                                _events?.currentScope.streamUnavailable == false &&
                                _profileCurrent
                        ? const CircularProgressIndicator()
                        : Text(
                            host != null && !_sameHost(saved, host) ? l.hostMetricsChanged : l.hostMetricsUnavailable,
                          ),
                  ),
          ),
        ],
      ),
    );
  }
}
