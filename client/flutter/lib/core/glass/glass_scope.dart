import 'dart:async';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/accessibility_bridge.dart';
import 'package:consolecrypt/core/glass/ambient_backdrop.dart';
import 'package:consolecrypt/core/glass/glass_backdrop.dart';
import 'package:consolecrypt/core/glass/glass_budget.dart';
import 'package:consolecrypt/core/glass/glass_mode_resolver.dart';
import 'package:consolecrypt/core/glass/glass_window.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// App-wide glass state published by [GlassScope].
@immutable
final class GlassScopeData {
  const GlassScopeData({
    required this.appearance,
    required this.budget,
    this.solidReason,
    this.refractionProgram,
    this.os = OsAccessibilitySignals.none,
    this.unifiedTitlebar = false,
    this.retryEffects,
  });

  final GlassAppearance appearance;
  final GlassSolidReason? solidReason;

  /// Live-backdrop budget (§6.6).
  final GlassBackdropBudget budget;

  /// Compiled refraction shader (macOS + Impeller), else `null`.
  final ui.FragmentProgram? refractionProgram;
  final OsAccessibilitySignals os;

  /// macOS unified title bar applied: toolbar controls need passthrough.
  final bool unifiedTitlebar;

  /// Available after automatic quality reduction, without changing the
  /// selected mode or overriding OS accessibility restrictions.
  final VoidCallback? retryEffects;

  GlassScopeData copyWith({
    GlassAppearance? appearance,
    GlassSolidReason? solidReason,
    bool clearSolidReason = false,
    GlassBackdropBudget? budget,
    ui.FragmentProgram? refractionProgram,
    bool clearRefractionProgram = false,
    OsAccessibilitySignals? os,
    bool? unifiedTitlebar,
    VoidCallback? retryEffects,
  }) => GlassScopeData(
    appearance: appearance ?? this.appearance,
    solidReason: clearSolidReason ? null : (solidReason ?? this.solidReason),
    budget: budget ?? this.budget,
    refractionProgram: clearRefractionProgram ? null : (refractionProgram ?? this.refractionProgram),
    os: os ?? this.os,
    unifiedTitlebar: unifiedTitlebar ?? this.unifiedTitlebar,
    retryEffects: retryEffects ?? this.retryEffects,
  );

  @override
  bool operator ==(Object other) =>
      other is GlassScopeData &&
      other.appearance == appearance &&
      other.solidReason == solidReason &&
      identical(other.budget, budget) &&
      identical(other.refractionProgram, refractionProgram) &&
      other.os == os &&
      other.unifiedTitlebar == unifiedTitlebar &&
      other.retryEffects == retryEffects;

  @override
  int get hashCode =>
      Object.hash(appearance, solidReason, budget, refractionProgram, os, unifiedTitlebar, retryEffects);
}

/// Debug frame timings are not representative. Tests can inject a guard
/// to exercise the release fallback through the real settings controls.
final glassFrameGuardFactoryProvider = Provider<GlassFrameGuard Function(VoidCallback)?>(
  (ref) => kProfileMode || kReleaseMode ? (onStepDown) => GlassFrameGuard(onStepDown: onStepDown) : null,
);

/// Publishes the effective glass appearance (tier, contrast, motion), the
/// live-blur budget and the refraction shader to the kit.
class GlassScope extends InheritedWidget {
  const GlassScope({required this.data, required super.child, super.key});

  final GlassScopeData data;

  static final GlassScopeData _fallback = GlassScopeData(
    appearance: GlassAppearance.fallback,
    budget: GlassBackdropBudget.fallback,
  );

  static GlassScopeData? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<GlassScope>()?.data;

  /// The nearest scope, or frosted defaults outside any scope.
  static GlassScopeData of(BuildContext context) => maybeOf(context) ?? _fallback;

  @override
  bool updateShouldNotify(GlassScope oldWidget) => data != oldWidget.data;
}

/// Root of the kit, placed in `MaterialApp.builder`: resolves the user's
/// Glass setting × OS accessibility (native bridge + `MediaQuery`) × the
/// frame guard into a [GlassScope], owns the backdrop budget and the
/// app-level [BackdropGroup], loads the refraction shader and syncs native
/// window chrome.
class GlassAppScope extends ConsumerStatefulWidget {
  const GlassAppScope({
    required this.child,
    super.key,
    this.unifiedTitlebar = false,
    this.ambient = false,
    this.localProfileCue = false,
  });

  final Widget child;

  /// macOS unified transparent title bar. Off until the phase-2 shell
  /// (floating `GlassSidebar` + `GlassToolbar`) reserves the traffic-light
  /// area and routes toolbar clicks through `MacosToolbarPassthrough`.
  final bool unifiedTitlebar;

  /// Paint [AmbientBackdrop] behind [child] (phase 2, with a transparent
  /// scaffold background).
  final bool ambient;

  /// Local-only profile (ADR-0106): the ambient backdrop's blob A turns warm
  /// graphite so the active profile is recognisable at a glance (§2.4).
  final bool localProfileCue;

  @override
  ConsumerState<GlassAppScope> createState() => _GlassAppScopeState();
}

class _GlassAppScopeState extends ConsumerState<GlassAppScope> with WidgetsBindingObserver {
  final GlassBackdropBudget _budget = GlassBackdropBudget();
  ui.FragmentProgram? _program;
  GlassTier _performanceCap = GlassTier.refractive;
  GlassFrameGuard? _guard;
  bool _unifiedTitlebar = false;
  Brightness? _captionBrightness;
  GlassMode? _lastMode;
  GlassTier _effectiveTier = GlassTier.solid;

  @override
  void initState() {
    super.initState();
    unawaited(
      GlassShaderProgram.load().then((program) {
        if (mounted && program != null) setState(() => _program = program);
      }),
    );
    _guard = ref.read(glassFrameGuardFactoryProvider)?.call(_stepDown);
    _guard?.setEnabled(false);
    _guard?.start();
    _budget.snapshot.addListener(_syncGuard);
    WidgetsBinding.instance.addObserver(this);
    if (widget.unifiedTitlebar) unawaited(_applyTitlebar(true));
  }

  @override
  void didUpdateWidget(GlassAppScope oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.unifiedTitlebar != widget.unifiedTitlebar) unawaited(_applyTitlebar(widget.unifiedTitlebar));
  }

  Future<void> _applyTitlebar(bool enabled) async {
    final ok = await GlassWindowChrome.setUnifiedTitlebar(enabled: enabled);
    if (mounted && ok) setState(() => _unifiedTitlebar = enabled);
  }

  void _stepDown() {
    if (!mounted || _effectiveTier == GlassTier.solid) return;
    setState(() {
      // Step down what is actually rendered (Windows / unavailable shaders
      // already use frosted), not the theoretical maximum tier.
      _performanceCap = switch (_effectiveTier) {
        GlassTier.refractive => GlassTier.frosted,
        GlassTier.frosted || GlassTier.solid => GlassTier.solid,
      };
    });
    _guard?.reset();
  }

  void _retryEffects() {
    setState(() => _performanceCap = GlassTier.refractive);
    _guard?.reset();
  }

  void _syncGuard() {
    final budget = _budget.current;
    final activeBlur =
        budget.suppression != GlassSuppression.all &&
        (budget.overlayLive > 0 || (budget.chromeLive > 0 && budget.suppression == GlassSuppression.none));
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    _guard?.setEnabled(
      _effectiveTier != GlassTier.solid && activeBlur && (lifecycle == null || lifecycle == AppLifecycleState.resumed),
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _syncGuard();
  }

  void _syncCaption(GlassTokens tokens) {
    if (defaultTargetPlatform != TargetPlatform.windows || _captionBrightness == tokens.brightness) return;
    _captionBrightness = tokens.brightness;
    SchedulerBinding.instance.addPostFrameCallback((_) {
      unawaited(GlassWindowChrome.setCaptionColors(caption: tokens.ambient.base, text: tokens.palette.label));
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _budget.snapshot.removeListener(_syncGuard);
    _guard?.stop();
    _budget.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final mode = ref.watch(glassModeProvider);
    if (_lastMode != mode) {
      _lastMode = mode;
      _performanceCap = GlassTier.refractive;
      _guard?.reset();
    }
    final os = ref.watch(osAccessibilityProvider);
    final media = MediaQuery.maybeOf(context);
    final effective = resolveEffectiveGlass(
      mode: mode,
      os: os,
      mediaHighContrast: media?.highContrast ?? false,
      mediaDisableAnimations: media?.disableAnimations ?? false,
      platform: defaultTargetPlatform,
      shaderFilterSupported: _program != null,
      performanceCap: _performanceCap,
    );
    if (_effectiveTier != effective.appearance.tier) _guard?.reset();
    _effectiveTier = effective.appearance.tier;
    _syncGuard();
    final tokens = GlassTokens.of(context);
    _syncCaption(tokens);
    Widget child = BackdropGroup(child: widget.child);
    if (widget.ambient) child = AmbientBackdrop(localProfileCue: widget.localProfileCue, child: child);
    return GlassScope(
      data: GlassScopeData(
        appearance: effective.appearance,
        solidReason: effective.solidReason,
        budget: _budget,
        refractionProgram: _program,
        os: os,
        unifiedTitlebar: _unifiedTitlebar,
        retryEffects: _performanceCap == GlassTier.refractive ? null : _retryEffects,
      ),
      child: child,
    );
  }
}

/// Frame-time guard: only measure while live blur is active in the
/// foreground, discard warm-up frames, and require sustained slow windows.
/// Changing the glass mode or explicitly retrying resets this session-only
/// cap; it must never become a sticky override of the user's preference.
class GlassFrameGuard {
  GlassFrameGuard({
    required this.onStepDown,
    this.window = 120,
    this.thresholdMs = 12,
    this.warmupFrames = 30,
    this.slowWindows = 2,
  }) : assert(window > 0),
       assert(warmupFrames >= 0),
       assert(slowWindows > 0) {
    reset();
  }

  final VoidCallback onStepDown;
  final int window;
  final double thresholdMs;
  final int warmupFrames;
  final int slowWindows;
  final List<Duration> _raster = [];
  bool _running = false;
  bool _enabled = true;
  int _warmupRemaining = 0;
  int _slowWindows = 0;

  void setEnabled(bool enabled) {
    if (_enabled == enabled) return;
    _enabled = enabled;
    reset();
  }

  void reset() {
    _raster.clear();
    _slowWindows = 0;
    _warmupRemaining = warmupFrames;
  }

  void start() {
    if (_running) return;
    _running = true;
    SchedulerBinding.instance.addTimingsCallback(_onTimings);
  }

  void stop() {
    if (!_running) return;
    _running = false;
    SchedulerBinding.instance.removeTimingsCallback(_onTimings);
  }

  void _onTimings(List<ui.FrameTiming> timings) => addSamples([for (final t in timings) t.rasterDuration]);

  /// Feeds raster durations (public for tests).
  void addSamples(Iterable<Duration> samples) {
    if (!_enabled) return;
    for (final sample in samples) {
      if (_warmupRemaining > 0) {
        _warmupRemaining--;
        continue;
      }
      _raster.add(sample);
      if (_raster.length >= window) {
        final over = percentile95(_raster).inMicroseconds > thresholdMs * 1000;
        _raster.clear();
        _slowWindows = over ? _slowWindows + 1 : 0;
        if (_slowWindows >= slowWindows) {
          reset();
          onStepDown();
          // Remaining samples were rendered with the previous quality;
          // never step down twice using the same delivered batch.
          return;
        }
      }
    }
  }

  /// 95th percentile (nearest rank).
  static Duration percentile95(List<Duration> samples) {
    if (samples.isEmpty) return Duration.zero;
    final sorted = [...samples]..sort();
    final rank = (0.95 * sorted.length).ceil().clamp(1, sorted.length);
    return sorted[rank - 1];
  }
}
