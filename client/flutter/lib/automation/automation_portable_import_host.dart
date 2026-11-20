import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_android_archive_picker.dart';
import 'package:consolecrypt/automation/automation_portable_import_controller.dart';
import 'package:consolecrypt/automation/automation_portable_import_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Staged composition only. Parent captures sticky original intent BEFORE any
/// optional dirty prompt and passes expectedUiEpoch/sourceCurrent. No production
/// capability, menu or generated import method is activated by this file.
Future<PortableInstalledProject?> showAutomationPortableImportReview({
  required BuildContext context,
  required WidgetRef ref,
  required AutomationScope originalScope,
  required AutomationService originalService,
  required bool Function() sourceCurrent,
  required PortableImportLabels labels,
  required PortableNativeGateway? scopeGateway,
  required PortableImportReviewGateway? reviewGateway,
  Listenable? sourceChanges,
  int? expectedUiEpoch,
}) async {
  final appServices = ref.read(appServicesProvider),
      files = ref.read(fileDialogServiceProvider),
      events = ref.read(criticalRuntimeEventsProvider);
  final profile = ref.read(activeProfileProvider)?.id.value, router = ref.read(routerProvider);
  if (profile == null || events == null) return null;
  final epoch = events.currentScope.epoch;
  bool original() =>
      context.mounted &&
      originalScope.unlocked &&
      identical(ref.read(appServicesProvider), appServices) &&
      identical(ref.read(automationScopeProvider), originalScope) &&
      identical(ref.read(automationServiceProvider), originalService) &&
      identical(ref.read(fileDialogServiceProvider), files) &&
      identical(ref.read(criticalRuntimeEventsProvider), events) &&
      ref.read(activeProfileProvider)?.id.value == profile &&
      events.currentScope.epoch == epoch &&
      !events.currentScope.streamUnavailable &&
      sourceCurrent() &&
      router.routeInformationProvider.value.uri.path == AppRoutes.automation;
  if (!original() || expectedUiEpoch != null && expectedUiEpoch != epoch) return null;
  final lifetime = PortableUiLifetime(profileId: profile, events: events, isOriginalUiCurrent: original);
  try {
    final result = await showAppDialog<PortableInstalledProject>(
      context,
      secure: true,
      builder: (_) => _ImportCaptureRoute(
        originalScope: originalScope,
        lifetime: lifetime,
        files: files,
        scopeGateway: scopeGateway,
        reviewGateway: reviewGateway,
        labels: labels,
        sourceChanges: sourceChanges,
      ),
    );
    return original() ? result : null; // receipt DATA only; never auto-select/open/run
  } finally {
    lifetime.dispose();
  }
}

class _ImportCaptureRoute extends ConsumerStatefulWidget {
  const _ImportCaptureRoute({
    required this.originalScope,
    required this.lifetime,
    required this.files,
    required this.scopeGateway,
    required this.reviewGateway,
    required this.labels,
    required this.sourceChanges,
  });
  final AutomationScope originalScope;
  final PortableUiLifetime lifetime;
  final FileDialogService files;
  final PortableNativeGateway? scopeGateway;
  final PortableImportReviewGateway? reviewGateway;
  final PortableImportLabels labels;
  final Listenable? sourceChanges;
  @override
  ConsumerState<_ImportCaptureRoute> createState() => _ImportCaptureState();
}

class _ImportCaptureState extends ConsumerState<_ImportCaptureRoute> {
  AutomationPortableNativeAdapter? _scope;
  AutomationPortableImportController? _controller;
  bool _failed = false, _unavailable = false, _closing = false;
  late final VoidCallback _unlisten;
  @override
  void initState() {
    super.initState();
    widget.lifetime.addListener(_changed);
    final route = ref.read(routerProvider).routeInformationProvider;
    void observe() {
      if (!widget.lifetime.current) widget.lifetime.retire();
    }

    route.addListener(observe);
    widget.sourceChanges?.addListener(observe);
    _unlisten = () {
      route.removeListener(observe);
      widget.sourceChanges?.removeListener(observe);
    };
    unawaited(_capture());
  }

  Future<void> _capture() async {
    try {
      if (widget.reviewGateway == null || widget.scopeGateway == null) {
        throw const PortableImportFailure(PortableImportIssue.unavailable);
      }
      final scope = await AutomationPortableNativeAdapter.capture(
        lifetime: widget.lifetime,
        gateway: widget.scopeGateway,
      );
      if (!mounted || _closing || !widget.lifetime.current) {
        scope.dispose();
        return;
      }
      final controller = AutomationPortableImportController(
        scope: scope.scope,
        gateway: widget.reviewGateway,
        files: widget.files,
        pickArchive: Platform.isAndroid ? pickAndroidAutomationArchive : null,
        lifetime: widget.lifetime,
        isOriginalCurrent: () => widget.lifetime.current,
      );
      setState(() {
        _scope = scope;
        _controller = controller;
      });
    } catch (error) {
      if (!mounted || _closing) return;
      if (!widget.lifetime.current) {
        _closeOwn();
        return;
      }
      setState(() {
        _failed = true;
        _unavailable = error is PortableImportFailure && error.issue == PortableImportIssue.unavailable;
      });
    }
  }

  void _changed() {
    if (mounted && !_closing) setState(() {});
  }

  void _closeOwn() {
    if (_closing) return;
    _closing = true;
    widget.lifetime.retire();
    _controller?.cancel(notify: false);
    final own = ModalRoute.of(context);
    if (own?.isActive == true) Navigator.of(context).removeRoute(own!);
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
        if (mounted) _closeOwn();
      });
      return const SizedBox.shrink();
    }
    final controller = _controller;
    if (controller != null) {
      return AutomationPortableImportDialog(
        controller: controller,
        originalScope: widget.originalScope,
        labels: widget.labels,
      );
    }
    return AlertDialog(
      title: Text(widget.labels.title),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (!_failed) const LinearProgressIndicator(),
          if (_failed)
            InfoBanner(
              message: widget.labels.issue(_unavailable ? PortableImportIssue.unavailable : PortableImportIssue.failed),
              tone: BannerTone.danger,
            ),
        ],
      ),
      actions: [
        TextButton(
          key: const ValueKey('portable-import-capture-close'),
          onPressed: _closeOwn,
          child: Text(widget.labels.cancel),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _closing = true;
    _unlisten();
    widget.lifetime.removeListener(_changed);
    _controller?.dispose();
    _scope?.dispose();
    super.dispose();
  }
}
