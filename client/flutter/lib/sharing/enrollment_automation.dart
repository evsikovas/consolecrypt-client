import 'dart:async';

import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// An online owner can process explicit Automatic grants while foreground and
/// unlocked. The core captures one session and revalidates the entire signed
/// chain; this scheduler neither creates grants nor confirms pairing codes.
class EnrollmentAutomationScope extends ConsumerStatefulWidget {
  const EnrollmentAutomationScope({required this.child, super.key});
  final Widget child;
  @override
  ConsumerState<EnrollmentAutomationScope> createState() => _EnrollmentAutomationScopeState();
}

class _EnrollmentAutomationScopeState extends ConsumerState<EnrollmentAutomationScope> with WidgetsBindingObserver {
  Timer? _timer;
  bool _busy = false, _foreground = true, _scheduled = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _timer = Timer.periodic(const Duration(seconds: 30), (_) => unawaited(_tick()));
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _foreground = state == AppLifecycleState.resumed;
    if (_foreground) unawaited(_tick());
  }

  Future<void> _tick() async {
    if (!mounted || _busy || !_foreground || ref.read(appServicesProvider).isMock) return;
    final scope = ref.read(sharingSessionScopeProvider);
    final status = ref.read(sharingStatusProvider).value;
    if (!sharingSessionCurrent(ref, scope) || status?.supportsOwnerOnlineEnrollment != true) return;
    _busy = true;
    try {
      final changes = await ref.read(enrollmentServiceProvider).processAutomatic();
      if (mounted && sharingSessionCurrent(ref, scope) && changes.isNotEmpty) refreshSharing(ref);
    } catch (_) {
      // A failed request never causes a new grant or a weaker fallback. The
      // next foreground pass can retry against fresh signed state.
    } finally {
      _busy = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(sharingSessionScopeProvider);
    final supported = ref.watch(sharingStatusProvider).value?.supportsOwnerOnlineEnrollment == true;
    if (supported && !_scheduled) {
      _scheduled = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        _scheduled = false;
        if (mounted) unawaited(_tick());
      });
    }
    return widget.child;
  }
}
