import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_awx_connection_host.dart';
import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_awx_l10n.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_browser.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_url.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

typedef AwxPortFactory = AwxOpaqueNativePort Function();
final automationAwxPortFactoryProvider = Provider<AwxPortFactory?>((ref) {
  final service = ref.watch(automationServiceProvider);
  return service is AutomationAwxPortCapability ? (service as AutomationAwxPortCapability).createAwxPort : null;
});

class AutomationAwxEntry extends ConsumerStatefulWidget {
  const AutomationAwxEntry({
    super.key,
    required this.scope,
    required this.service,
    required this.events,
    required this.portFactory,
    required this.active,
  });
  final AutomationScope scope;
  final AutomationService service;
  final RuntimeNoticeEvents events;
  final AwxPortFactory portFactory;
  final bool active;
  @override
  ConsumerState<AutomationAwxEntry> createState() => _AutomationAwxEntryState();
}

class _AutomationAwxEntryState extends ConsumerState<AutomationAwxEntry> {
  late AwxController _owner;
  late AwxHandleGateway _gateway;
  bool _alive = true, _viewActive = false, _openFailed = false;

  @override
  void initState() {
    super.initState();
    _viewActive = widget.active;
    _createOwner();
  }

  void _createOwner() {
    // Capture UI identity/epoch synchronously BEFORE the first Native await.
    // The hub nonce is deliberately never used as Native scope authority.
    final scope = widget.scope, service = widget.service, events = widget.events;
    final appServices = ref.read(appServicesProvider);
    final epoch = events.currentScope.epoch;
    final profile = ref.read(activeProfileProvider)?.id.value;
    bool original() =>
        _alive &&
        mounted &&
        profile != null &&
        scope.unlocked &&
        identical(ref.read(appServicesProvider), appServices) &&
        identical(ref.read(automationScopeProvider), scope) &&
        identical(ref.read(automationServiceProvider), service) &&
        identical(ref.read(criticalRuntimeEventsProvider), events) &&
        ref.read(activeProfileProvider)?.id.value == profile &&
        events.currentScope.epoch == epoch &&
        !events.currentScope.streamUnavailable;
    _gateway = AwxHandleGateway(profileId: profile ?? '', isOriginalCurrent: original, port: widget.portFactory());
    _owner = AwxController(
      profileId: profile ?? '',
      events: events,
      isOriginalUiCurrent: original,
      operationId: generateUuidV4,
      gateway: _gateway,
    );
    _owner.addListener(_changed);
  }

  void _changed() {
    if (_owner.retired) unawaited(_gateway.close());
  }

  void _disposeOwner({bool afterBuild = false}) {
    final owner = _owner;
    owner.removeListener(_changed);
    // Close admission/cancel original issued reads NOW, even if unrelated
    // modal listeners may only be notified outside this widget build.
    unawaited(_gateway.close());
    if (afterBuild) {
      owner.cancelRead(deferNotification: true);
      scheduleMicrotask(owner.dispose);
    } else {
      owner.dispose();
    }
  }

  @override
  void didUpdateWidget(AutomationAwxEntry oldWidget) {
    super.didUpdateWidget(oldWidget);
    _viewActive = widget.active;
    if (!identical(oldWidget.scope, widget.scope) ||
        !identical(oldWidget.service, widget.service) ||
        !identical(oldWidget.events, widget.events) ||
        !identical(oldWidget.portFactory, widget.portFactory)) {
      _disposeOwner(afterBuild: true);
      _createOwner();
    } else if (oldWidget.active && !widget.active) {
      // Notify the OWNED modal guards only after visibility is false. This
      // cancels local reads/discards tickets; it never cancels an AWX job.
      _owner.cancelRead(deferNotification: true);
    }
  }

  bool get _visibleCurrent => _alive && mounted && _viewActive && _owner.current;
  Future<void> _openResource(String originalNativeUrl) async {
    if (!_visibleCurrent) return;
    final uri = reviewGrafanaBrowserUri(originalNativeUrl);
    final owner = _owner;
    bool current() => _visibleCurrent && identical(owner, _owner);
    var opened = false;
    if (uri != null && uri.scheme == 'https' && !uri.hasQuery && !uri.hasFragment) {
      try {
        opened = await ref.read(monitoringGrafanaBrowserLauncherProvider)(uri);
      } catch (_) {
        // Fixed UI copy only; no browser/native diagnostic or URL disclosure.
      }
    }
    if (current()) setState(() => _openFailed = !opened);
  }

  void _refreshRetired() {
    if (!_alive || !mounted || !_viewActive) return;
    _disposeOwner();
    setState(() {
      _openFailed = false;
      _createOwner();
    });
    unawaited(_owner.loadCatalog()); // One explicit user refresh, no timer.
  }

  @override
  Widget build(BuildContext context) {
    // Reactive original provider replacements cannot leave a cached branch
    // usable while their old stream callback is still queued.
    ref.watch(appServicesProvider);
    ref.watch(automationScopeProvider);
    ref.watch(automationServiceProvider);
    ref.watch(criticalRuntimeEventsProvider);
    ref.watch(activeProfileProvider);
    if (!_owner.current && !_owner.retired) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_alive && mounted && !_owner.current) _owner.retire();
      });
    }
    final l = context.l10n;
    return AnimatedBuilder(
      animation: _owner,
      builder: (context, _) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_openFailed)
            InfoBanner(
              key: const ValueKey('awx-resource-open-failed'),
              message: l.awxResourceOpenFailed,
              tone: BannerTone.danger,
            ),
          if (_owner.retired)
            Align(
              alignment: Alignment.centerLeft,
              child: GlassButton(
                key: const ValueKey('awx-reopen'),
                label: l.awxRefreshConnections,
                onPressed: _viewActive ? _refreshRetired : null,
              ),
            ),
          Expanded(
            child: AwxConnectionWorkspace(
              controller: _owner,
              copy: (key) => localizedAwxCopy(l, key),
              httpsNotice: l.awxHttpsConnectionHelp,
              isViewActive: () => _visibleCurrent,
              onOpenResource: _openResource,
            ),
          ),
        ],
      ),
    );
  }

  @override
  void dispose() {
    _alive = false;
    _viewActive = false;
    _disposeOwner(afterBuild: true);
    super.dispose();
  }
}
