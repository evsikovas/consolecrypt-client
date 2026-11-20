import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/terminal/host_metrics_view.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final hostMetricsReaderProvider = Provider<HostMetricsReader?>(
  (ref) => ref.watch(appServicesProvider.select((app) => app.hostMetrics)),
);

final activeSshTerminalProvider = Provider<TerminalTab?>((ref) {
  return ref.watch(
    terminalTabsProvider.select((tabs) {
      final tab = tabs.active;
      return tab != null && tab.isConnected && !tab.exited && tab.host != null ? tab : null;
    }),
  );
});

class TerminalServerPanel extends ConsumerStatefulWidget {
  const TerminalServerPanel({super.key, required this.tab});

  final TerminalTab tab;

  @override
  ConsumerState<TerminalServerPanel> createState() => _TerminalServerPanelState();
}

class _TerminalServerPanelState extends ConsumerState<TerminalServerPanel> {
  late final _profile = ref.read(criticalNoticeScopeProvider);
  late final _events = ref.read(criticalRuntimeEventsProvider);
  StreamSubscription<RuntimeNoticeFrame>? _subscription;
  RuntimeNoticeFrame? _origin;

  @override
  void initState() {
    super.initState();
    _origin = _frame;
    _subscription = _events?.watchRuntimeNotices().listen(
      (_) {
        if (!mounted) return;
        final frame = _frame;
        if (frame?.epoch == _origin?.epoch && (frame == null || frame.scope!.same(_origin?.scope))) return;
        setState(() => _origin = frame);
      },
      onError: (Object _) => _clearScope(),
      onDone: _clearScope,
    );
  }

  RuntimeNoticeFrame? get _frame {
    final frame = _events?.currentScope;
    return frame != null &&
            !frame.streamUnavailable &&
            frame.scope != null &&
            frame.scope!.profileId == _profile.profileId
        ? frame
        : null;
  }

  void _clearScope() {
    if (mounted && _origin != null) setState(() => _origin = null);
  }

  @override
  void dispose() {
    unawaited(_subscription?.cancel());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final profile = ref.watch(criticalNoticeScopeProvider);
    final events = ref.watch(criticalRuntimeEventsProvider);
    final reader = ref.watch(hostMetricsReaderProvider);
    final origin = _origin;
    if (!_profile.unlocked || !identical(profile, _profile) || !identical(events, _events)) {
      return const SizedBox.shrink();
    }

    bool current() {
      if (!mounted ||
          !identical(ref.read(criticalNoticeScopeProvider), _profile) ||
          !identical(ref.read(criticalRuntimeEventsProvider), _events) ||
          !identical(ref.read(hostMetricsReaderProvider), reader) ||
          !identical(ref.read(activeSshTerminalProvider), widget.tab) ||
          ref.read(workspaceToolsProvider).selected != WorkspaceTool.server) {
        return false;
      }
      final frame = _frame;
      return origin != null && frame?.epoch == origin.epoch && origin.scope!.same(frame?.scope);
    }

    return HostMetricsView(
      key: ValueKey(origin?.epoch),
      host: widget.tab.host!,
      sessionId: widget.tab.sessionId,
      reader: reader,
      scope: origin?.scope,
      scopePending: origin == null && events?.currentScope.streamUnavailable == false,
      isCurrent: current,
    );
  }
}
