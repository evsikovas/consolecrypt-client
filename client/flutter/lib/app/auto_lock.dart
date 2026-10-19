import 'dart:async';

import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Enforces the device-local inactivity setting for the unlocked vault.
///
/// The timer continues while the window is hidden when the OS runs the Dart
/// isolate. Mobile suspension can stop timers, so foregrounding checks the
/// original deadline before accepting fresh activity.
class AutoLockScope extends ConsumerStatefulWidget {
  const AutoLockScope({required this.child, this.now, this.elapsed, super.key});

  final Widget child;

  /// Clock overrides for deterministic lifecycle tests.
  final DateTime Function()? now;
  final Duration Function()? elapsed;

  /// Native text-input clients (for example the remote desktop keyboard)
  /// report edits here because software keyboards do not emit hardware keys.
  static void recordActivity(BuildContext context) {
    context.getInheritedWidgetOfExactType<_ActivityScope>()?.activity();
  }

  @override
  ConsumerState<AutoLockScope> createState() => _AutoLockScopeState();
}

class _AutoLockScopeState extends ConsumerState<AutoLockScope> with WidgetsBindingObserver {
  final _stopwatch = Stopwatch();
  Timer? _timer;
  ({String profile, String vault})? _session;
  Duration _timeout = Duration.zero;
  DateTime? _lastWall;
  Duration? _lastElapsed;
  int _generation = 0;
  int? _lockingGeneration;
  TextEditingController? _editingController;

  DateTime get _now => widget.now?.call() ?? DateTime.now();
  Duration get _elapsed => widget.elapsed?.call() ?? _stopwatch.elapsed;

  @override
  void initState() {
    super.initState();
    _stopwatch.start();
    WidgetsBinding.instance.addObserver(this);
    HardwareKeyboard.instance.addHandler(_onKey);
    FocusManager.instance.addListener(_onFocus);
    ref.listenManual(profilesProvider, (_, _) => _configure(), fireImmediately: true);
    ref.listenManual(vaultStatusProvider, (_, _) => _configure(), fireImmediately: true);
    ref.listenManual(localSettingsProvider, (_, _) => _configure(), fireImmediately: true);
  }

  void _configure() {
    if (!mounted) return;
    final profile = ref.read(profileServiceProvider).currentProfiles.active;
    final vault = ref.read(vaultServiceProvider).currentStatus;
    final minutes = ref.read(settingsServiceProvider).currentLocal.autoLockMinutes;
    final session = profile != null && vault.isUnlocked && vault.vaultId != null && minutes > 0
        ? (profile: profile.id.value, vault: vault.vaultId!.value)
        : null;
    final timeout = minutes > 0 ? Duration(minutes: minutes) : Duration.zero;
    if (session != _session) {
      _generation++;
      _session = session;
      _lastWall = session == null ? null : _now;
      _lastElapsed = session == null ? null : _elapsed;
    }
    _timeout = timeout;
    _schedule();
  }

  Duration get _idle {
    final wall = _now.difference(_lastWall!);
    final monotonic = _elapsed - _lastElapsed!;
    // Wall time includes OS suspension; monotonic time prevents a backward
    // clock adjustment from extending an already running inactivity period.
    return wall > monotonic ? wall : monotonic;
  }

  void _schedule() {
    _timer?.cancel();
    _timer = null;
    if (_session == null || _lockingGeneration == _generation) return;
    final remaining = _timeout - _idle;
    final generation = _generation;
    _timer = Timer(remaining.isNegative ? Duration.zero : remaining, () {
      if (!mounted || generation != _generation) return;
      _checkDeadline();
    });
  }

  void _checkDeadline() {
    if (_session == null || _lockingGeneration == _generation) return;
    if (_idle < _timeout) {
      _schedule();
      return;
    }
    unawaited(_lock());
  }

  Future<void> _lock() async {
    final generation = _generation;
    final expected = _session;
    if (expected == null || _lockingGeneration == generation) return;
    final vault = ref.read(vaultServiceProvider);
    // Read current services, not a possibly delayed provider stream snapshot.
    if (ref.read(profileServiceProvider).currentProfiles.active?.id.value != expected.profile ||
        vault.currentStatus.vaultId?.value != expected.vault ||
        !vault.currentStatus.isUnlocked ||
        ref.read(settingsServiceProvider).currentLocal.autoLockMinutes <= 0) {
      _configure();
      return;
    }
    _timer?.cancel();
    _timer = null;
    _lockingGeneration = generation;
    try {
      await vault.lock();
    } on Object {
      // Keep the original deadline. A transient failure must not silently
      // disable protection; retry without spinning or leaking error details.
      if (mounted && generation == _generation) {
        _timer = Timer(const Duration(seconds: 1), _checkDeadline);
      }
    } finally {
      if (_lockingGeneration == generation) _lockingGeneration = null;
    }
  }

  void _activity() {
    if (_session == null || _lockingGeneration == _generation) return;
    // A delayed first input after suspension cannot rescue an expired vault.
    if (_idle >= _timeout) {
      _checkDeadline();
      return;
    }
    _lastWall = _now;
    _lastElapsed = _elapsed;
    _schedule();
  }

  bool _onKey(KeyEvent event) {
    _activity();
    return false; // Observe even handled RDP keys without consuming them.
  }

  void _onFocus() {
    final controller = FocusManager.instance.primaryFocus?.context
        ?.findAncestorStateOfType<EditableTextState>()
        ?.widget
        .controller;
    if (identical(controller, _editingController)) return;
    _editingController?.removeListener(_activity);
    _editingController = controller;
    controller?.addListener(_activity);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) _checkDeadline();
  }

  @override
  void dispose() {
    _generation++;
    _timer?.cancel();
    _editingController?.removeListener(_activity);
    FocusManager.instance.removeListener(_onFocus);
    HardwareKeyboard.instance.removeHandler(_onKey);
    WidgetsBinding.instance.removeObserver(this);
    _stopwatch.stop();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _ActivityScope(
    activity: _activity,
    child: Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: (_) => _activity(),
      onPointerMove: (_) => _activity(),
      onPointerHover: (_) => _activity(),
      onPointerSignal: (_) => _activity(),
      onPointerPanZoomStart: (_) => _activity(),
      onPointerPanZoomUpdate: (_) => _activity(),
      child: widget.child,
    ),
  );
}

class _ActivityScope extends InheritedWidget {
  const _ActivityScope({required this.activity, required super.child});

  final VoidCallback activity;

  @override
  bool updateShouldNotify(_ActivityScope oldWidget) => false;
}
