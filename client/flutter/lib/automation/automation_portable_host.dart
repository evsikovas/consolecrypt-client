import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final class PortableHostLabels {
  const PortableHostLabels({required this.dialog, required this.preparing, required this.unavailable});
  final PortableDialogLabels dialog;
  final String preparing, unavailable;
}

/// Explicit menu action only. Nothing calls this while opening a project/menu.
/// Native gateway is optional until its original registered-handle seam exists.
Future<void> showAutomationPortableOperation(
  BuildContext context,
  WidgetRef ref, {
  required AutomationScope originalScope,
  required AutomationService originalService,
  required bool Function() sourceCurrent,
  required PortableHostLabels labels,
  required bool importMode,
  PortableNativeGateway? gateway,
  String? projectId,
  int? projectRevision,
  Listenable? sourceChanges,
  int? expectedUiEpoch,
}) async {
  final appServices = ref.read(appServicesProvider);
  final files = ref.read(fileDialogServiceProvider);
  final events = ref.read(criticalRuntimeEventsProvider);
  final profile = ref.read(activeProfileProvider)?.id.value;
  final router = ref.read(routerProvider);
  // All identity and original epoch capture precedes showDialog's first await.
  // The observer nonce never supplies the Native scope or export permission.
  bool original() =>
      context.mounted &&
      originalScope.unlocked &&
      profile != null &&
      originalScope.profileId == profile &&
      identical(ref.read(appServicesProvider), appServices) &&
      identical(ref.read(automationScopeProvider), originalScope) &&
      identical(ref.read(automationServiceProvider), originalService) &&
      identical(ref.read(fileDialogServiceProvider), files) &&
      identical(ref.read(criticalRuntimeEventsProvider), events) &&
      ref.read(activeProfileProvider)?.id.value == profile &&
      sourceCurrent() &&
      router.routeInformationProvider.value.uri.path == AppRoutes.automation;
  if (!original() || events == null || expectedUiEpoch != null && events.currentScope.epoch != expectedUiEpoch) return;
  final lifetime = PortableUiLifetime(profileId: profile!, events: events, isOriginalUiCurrent: original);
  try {
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (_) => _PortableCaptureRoute(
        originalScope: originalScope,
        lifetime: lifetime,
        files: files,
        gateway: gateway,
        labels: labels,
        importMode: importMode,
        projectId: projectId,
        projectRevision: projectRevision,
        sourceChanges: sourceChanges,
      ),
    );
  } finally {
    // Widget route normally owns disposal. If its builder never mounted,
    // retirement still closes UI admission; no user pathname is ever deleted.
    lifetime.dispose();
  }
}

class _PortableCaptureRoute extends ConsumerStatefulWidget {
  const _PortableCaptureRoute({
    required this.originalScope,
    required this.lifetime,
    required this.files,
    required this.gateway,
    required this.labels,
    required this.importMode,
    required this.projectId,
    required this.projectRevision,
    required this.sourceChanges,
  });
  final AutomationScope originalScope;
  final PortableUiLifetime lifetime;
  final FileDialogService files;
  final PortableNativeGateway? gateway;
  final PortableHostLabels labels;
  final bool importMode;
  final String? projectId;
  final int? projectRevision;
  final Listenable? sourceChanges;
  @override
  ConsumerState<_PortableCaptureRoute> createState() => _PortableCaptureRouteState();
}

class _PortableCaptureRouteState extends ConsumerState<_PortableCaptureRoute> {
  AutomationPortableNativeAdapter? _adapter;
  AutomationPortableController? _controller;
  bool _failed = false, _unavailable = false, _closing = false;
  late final VoidCallback _removeRouteListener;
  @override
  void initState() {
    super.initState();
    widget.lifetime.addListener(_changed);
    final route = ref.read(routerProvider).routeInformationProvider;
    void routeChanged() {
      if (!widget.lifetime.current) widget.lifetime.retire();
    }

    route.addListener(routeChanged);
    widget.sourceChanges?.addListener(routeChanged);
    _removeRouteListener = () {
      route.removeListener(routeChanged);
      widget.sourceChanges?.removeListener(routeChanged);
    };
    unawaited(_capture());
  }

  void _changed() {
    if (mounted && !_closing) setState(() {});
  }

  Future<void> _capture() async {
    try {
      final adapter = await AutomationPortableNativeAdapter.capture(lifetime: widget.lifetime, gateway: widget.gateway);
      if (!mounted || _closing || !widget.lifetime.current) {
        adapter.dispose();
        return;
      }
      final controller = adapter.createController(
        files: widget.files,
        projectId: widget.projectId,
        projectRevision: widget.projectRevision,
      );
      setState(() {
        _adapter = adapter;
        _controller = controller;
      });
    } catch (error) {
      if (mounted && !_closing && !widget.lifetime.current) {
        _closeOwnRoute();
        return;
      }
      if (mounted && !_closing && widget.lifetime.current) {
        setState(() {
          _failed = true;
          _unavailable = error is PortablePackageFailure && error.code == 'portable_backend_unavailable';
        });
      }
    }
  }

  void _closeOwnRoute() {
    if (_closing) return;
    _closing = true;
    widget.lifetime.retire();
    _controller?.cancel(notify: false);
    final route = ModalRoute.of(context);
    if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(appServicesProvider);
    ref.watch(automationScopeProvider);
    ref.watch(automationServiceProvider);
    ref.watch(fileDialogServiceProvider);
    ref.watch(criticalRuntimeEventsProvider);
    ref.watch(activeProfileProvider);
    if (!widget.lifetime.current) {
      _controller?.cancel(notify: false);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _closeOwnRoute();
      });
      return const SizedBox.shrink();
    }
    final controller = _controller, labels = widget.labels;
    if (controller != null) {
      return AutomationPortableDialog(
        controller: controller,
        originalScope: widget.originalScope,
        labels: labels.dialog,
        importMode: widget.importMode,
      );
    }
    return AlertDialog(
      title: Text(widget.importMode ? labels.dialog.importTitle : labels.dialog.exportTitle),
      content: SizedBox(
        width: 620,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (!_failed) ...[
              const LinearProgressIndicator(key: ValueKey('portable-capture-pending')),
              Text(labels.preparing),
            ],
            if (_failed)
              InfoBanner(
                key: const ValueKey('portable-capture-failure'),
                message: _unavailable ? labels.unavailable : labels.dialog.failed,
                tone: BannerTone.danger,
              ),
          ],
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('portable-capture-cancel'),
          onPressed: _closeOwnRoute,
          child: Text(labels.dialog.cancel),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _closing = true;
    _removeRouteListener();
    widget.lifetime.removeListener(_changed);
    // Existing child dialog owns controller disposal/wiping. The original
    // lifetime independently cancels a pending registration/worker as it retires.
    final adapter = _adapter;
    if (adapter == null) {
      widget.lifetime.dispose();
    } else {
      adapter.dispose();
    }
    super.dispose();
  }
}
