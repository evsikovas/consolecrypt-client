import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';

/// Which slot a live surface asks for (LIQUID_GLASS_SPEC §6.6 budget).
enum GlassBackdropKind {
  /// Persistent chrome (the toolbar capsule group on list routes).
  chrome,

  /// One transient overlay level (menu, popover, dialog, toast stack).
  overlay,
}

/// How far live blur is currently suppressed.
enum GlassSuppression {
  none,

  /// Terminal / SFTP / AI routes: persistent chrome renders static.
  chrome,

  /// Device approval and other secure flows: no blur anywhere on screen.
  all,
}

/// Counts shown by the gallery's performance overlay.
@immutable
final class GlassBudgetSnapshot {
  const GlassBudgetSnapshot({
    this.chromeLive = 0,
    this.overlayLive = 0,
    this.denied = 0,
    this.refractive = 0,
    this.suppression = GlassSuppression.none,
  });

  final int chromeLive;
  final int overlayLive;

  /// Surfaces that asked for a live backdrop but were refused (they render
  /// static, or solid when they are overlays).
  final int denied;
  final int refractive;
  final GlassSuppression suppression;

  int get live => chromeLive + overlayLive;

  @override
  bool operator ==(Object other) =>
      other is GlassBudgetSnapshot &&
      other.chromeLive == chromeLive &&
      other.overlayLive == overlayLive &&
      other.denied == denied &&
      other.refractive == refractive &&
      other.suppression == suppression;

  @override
  int get hashCode => Object.hash(chromeLive, overlayLive, denied, refractive, suppression);

  @override
  String toString() =>
      'live $live (chrome $chromeLive, overlay $overlayLive) · denied $denied · '
      'refractive $refractive · suppression ${suppression.name}';
}

/// Caps simultaneous live `BackdropFilter`s (§6.6): at most [maxChrome]
/// persistent chrome surface and [maxOverlay] overlay level. Surfaces that
/// are refused fall back to static glass (chrome) or the solid tier
/// (overlays), so a second blur is never paid for.
///
/// One budget exists per app (owned by `GlassAppScope`). Listeners are
/// notified after the frame, never during build.
class GlassBackdropBudget {
  /// [parent]: a nested budget (e.g. a demo area with more slots) still
  /// obeys the parent's suppression — device approval stops *all* blur.
  GlassBackdropBudget({this.maxChrome = 1, this.maxOverlay = 1, this.parent}) {
    parent?.suppression.addListener(_syncEffective);
  }

  /// Used by surfaces outside any `GlassScope` (isolated widget tests).
  static final GlassBackdropBudget fallback = GlassBackdropBudget();

  final int maxChrome;
  final int maxOverlay;
  final GlassBackdropBudget? parent;

  final Set<Object> _chrome = {};
  final Set<Object> _overlay = {};
  final Set<Object> _denied = {};
  final Set<Object> _refractive = {};
  final Set<Object> _suppressChrome = {};
  final Set<Object> _suppressAll = {};

  final ValueNotifier<GlassSuppression> _suppression = ValueNotifier(GlassSuppression.none);
  late final ValueNotifier<GlassSuppression> _effective = ValueNotifier(_mergedSuppression());
  final ValueNotifier<GlassBudgetSnapshot> _snapshot = ValueNotifier(const GlassBudgetSnapshot());
  bool _flushScheduled = false;
  bool _disposed = false;

  /// Current suppression level, including the parent's (updated post-frame).
  ValueListenable<GlassSuppression> get suppression => _effective;

  GlassSuppression _mergedSuppression() {
    final own = _suppression.value;
    final inherited = parent?.suppression.value ?? GlassSuppression.none;
    return own.index >= inherited.index ? own : inherited;
  }

  void _syncEffective() {
    if (_disposed) return;
    _effective.value = _mergedSuppression();
  }

  /// Live counts (updated post-frame).
  ValueListenable<GlassBudgetSnapshot> get snapshot => _snapshot;

  /// Synchronous view of the counts (tests).
  GlassBudgetSnapshot get current => _compute();

  /// Asks for a live slot for [owner]. Returns whether it was granted;
  /// asking again for the same owner is idempotent.
  bool acquire(Object owner, GlassBackdropKind kind) {
    final set = kind == GlassBackdropKind.chrome ? _chrome : _overlay;
    final max = kind == GlassBackdropKind.chrome ? maxChrome : maxOverlay;
    if (set.contains(owner)) return true;
    final granted = set.length < max;
    if (granted) {
      set.add(owner);
      _denied.remove(owner);
    } else {
      _denied.add(owner);
    }
    _scheduleFlush();
    return granted;
  }

  /// Returns [owner]'s slot (if any).
  void release(Object owner) {
    final changed = _chrome.remove(owner) | _overlay.remove(owner) | _denied.remove(owner) | _refractive.remove(owner);
    if (changed) _scheduleFlush();
  }

  /// Records whether [owner] currently runs the refraction shader.
  void markRefractive(Object owner, {required bool refractive}) {
    final changed = refractive ? _refractive.add(owner) : _refractive.remove(owner);
    if (changed) _scheduleFlush();
  }

  /// Starts suppressing live blur on behalf of [owner].
  void suppress(Object owner, {required bool includeOverlays}) {
    final changed = includeOverlays
        ? (_suppressAll.add(owner) | _suppressChrome.remove(owner))
        : (_suppressChrome.add(owner) | _suppressAll.remove(owner));
    if (changed) _scheduleFlush();
  }

  void unsuppress(Object owner) {
    final changed = _suppressAll.remove(owner) | _suppressChrome.remove(owner);
    if (changed) _scheduleFlush();
  }

  GlassSuppression get _currentSuppression => _suppressAll.isNotEmpty
      ? GlassSuppression.all
      : _suppressChrome.isNotEmpty
      ? GlassSuppression.chrome
      : GlassSuppression.none;

  GlassBudgetSnapshot _compute() => GlassBudgetSnapshot(
    chromeLive: _chrome.length,
    overlayLive: _overlay.length,
    denied: _denied.length,
    refractive: _refractive.length,
    suppression: _currentSuppression,
  );

  void _scheduleFlush() {
    if (_flushScheduled || _disposed) return;
    _flushScheduled = true;
    final binding = SchedulerBinding.instance;
    binding.addPostFrameCallback((_) => _flush(), debugLabel: 'GlassBackdropBudget.flush');
    // Changes outside a frame (e.g. a dispose after the last frame) still
    // need the callback to run.
    if (binding.schedulerPhase == SchedulerPhase.idle) binding.scheduleFrame();
  }

  void _flush() {
    _flushScheduled = false;
    if (_disposed) return;
    _suppression.value = _currentSuppression;
    _syncEffective();
    _snapshot.value = _compute();
  }

  void dispose() {
    _disposed = true;
    parent?.suppression.removeListener(_syncEffective);
    _suppression.dispose();
    _effective.dispose();
    _snapshot.dispose();
  }
}

/// Suppresses live blur while mounted and visible (ticker mode enabled):
///
/// * `includeOverlays: false` — Terminal / SFTP / AI Chat routes keep all
///   chrome static (§3 rule 1);
/// * `includeOverlays: true` — device approval: no blur anywhere (§4.13).
///
/// It has no visual output and no animation.
class GlassBlurSuppressor extends StatefulWidget {
  const GlassBlurSuppressor({required this.child, required this.budget, super.key, this.includeOverlays = false});

  final Widget child;
  final GlassBackdropBudget budget;
  final bool includeOverlays;

  @override
  State<GlassBlurSuppressor> createState() => _GlassBlurSuppressorState();
}

class _GlassBlurSuppressorState extends State<GlassBlurSuppressor> {
  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(GlassBlurSuppressor oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.budget != widget.budget) oldWidget.budget.unsuppress(this);
    _sync();
  }

  void _sync() {
    // Offstage shell branches run with tickers disabled: they must not
    // suppress blur for the visible route.
    if (TickerMode.valuesOf(context).enabled) {
      widget.budget.suppress(this, includeOverlays: widget.includeOverlays);
    } else {
      widget.budget.unsuppress(this);
    }
  }

  @override
  void dispose() {
    widget.budget.unsuppress(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
