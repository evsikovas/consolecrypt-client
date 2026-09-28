import 'package:flutter/animation.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/physics.dart';

/// Motion tokens (LIQUID_GLASS_SPEC §2.9). Under Reduce Motion everything
/// becomes an opacity-only fade of [fast]; see [GlassMotion.reduced].
@immutable
final class GlassMotion {
  const GlassMotion({this.reduceMotion = false});

  final bool reduceMotion;

  /// Press-down, hotspot rise.
  static const instant = Duration(milliseconds: 90);

  /// Hover, dismiss, Reduce Motion fades.
  static const fast = Duration(milliseconds: 120);

  /// Menus, toggles, badges.
  static const base = Duration(milliseconds: 180);

  /// Segmented thumb, tab switch, materialize.
  static const medium = Duration(milliseconds: 240);

  /// Dialogs, palette, morph.
  static const slow = Duration(milliseconds: 320);

  /// Dematerialize.
  static const dematerialize = Duration(milliseconds: 150);

  /// Hotspot decay after a press.
  static const hotspotDecay = Duration(milliseconds: 240);

  static const Curve appear = Cubic(0.2, 0, 0, 1);
  static const Curve dismiss = Cubic(0.4, 0, 1, 1);

  static final SpringDescription springSmooth = SpringDescription.withDurationAndBounce(
    duration: const Duration(milliseconds: 350),
  );
  static final SpringDescription springSnappy = SpringDescription.withDurationAndBounce(
    duration: const Duration(milliseconds: 300),
    bounce: 0.12,
  );
  static final SpringDescription springBouncy = SpringDescription.withDurationAndBounce(
    duration: const Duration(milliseconds: 400),
    bounce: 0.22,
  );

  /// Press scale for controls ≤ 44 high / panels.
  static const double pressScaleControl = 0.97;
  static const double pressScalePanel = 0.99;

  /// Materialize start values (§2.9).
  static const double materializeScale = 0.96;
  static const double materializeOffset = 4;

  /// Maximum pointer tilt of the rim gradient (degrees).
  static const double maxTiltDegrees = 10;

  /// Duration to use for an effect: [fast] fade under Reduce Motion.
  Duration resolve(Duration full) => reduceMotion ? fast : full;

  /// Movement (scale/offset/spring/morph) is disabled under Reduce Motion.
  bool get allowsMovement => !reduceMotion;

  Curve get appearCurve => reduceMotion ? Curves.linear : appear;

  /// A curve that follows [spring] over [duration] (for implicit
  /// animations that only take a curve), overshoot included.
  static Curve springCurve(SpringDescription spring, Duration duration) => _SpringCurve(spring, duration);
}

final class _SpringCurve extends Curve {
  _SpringCurve(this.spring, this.duration) : _simulation = SpringSimulation(spring, 0, 1, 0);

  final SpringDescription spring;
  final Duration duration;
  final SpringSimulation _simulation;

  @override
  double transformInternal(double t) {
    final seconds = t * duration.inMicroseconds / Duration.microsecondsPerSecond;
    return _simulation.x(seconds);
  }
}
