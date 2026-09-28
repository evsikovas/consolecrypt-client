import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:flutter/foundation.dart';

/// Display/accessibility state read natively by the runners over the
/// `consolecrypt/accessibility` channel (LIQUID_GLASS_SPEC §6.8). Flutter
/// exposes none of these on macOS and only high contrast on Windows.
@immutable
final class OsAccessibilitySignals {
  const OsAccessibilitySignals({
    this.reduceTransparency = false,
    this.increaseContrast = false,
    this.reduceMotion = false,
    this.differentiateWithoutColor = false,
    this.remoteSession = false,
    this.batterySaver = false,
  });

  /// macOS Reduce Transparency; Windows "Transparency effects" off.
  final bool reduceTransparency;

  /// macOS Increase Contrast; Windows high-contrast theme.
  final bool increaseContrast;

  /// macOS Reduce Motion; Windows "Animation effects" off.
  final bool reduceMotion;
  final bool differentiateWithoutColor;

  /// Windows Remote Desktop session (`SM_REMOTESESSION`).
  final bool remoteSession;

  /// Windows battery saver / macOS Low Power Mode.
  final bool batterySaver;

  static const none = OsAccessibilitySignals();

  factory OsAccessibilitySignals.fromMap(Map<Object?, Object?>? map) {
    bool flag(String key) => map?[key] == true;
    return OsAccessibilitySignals(
      reduceTransparency: flag('reduceTransparency'),
      increaseContrast: flag('increaseContrast'),
      reduceMotion: flag('reduceMotion'),
      differentiateWithoutColor: flag('differentiateWithoutColor'),
      remoteSession: flag('remoteSession'),
      batterySaver: flag('batterySaver'),
    );
  }

  Map<String, bool> toMap() => {
    'reduceTransparency': reduceTransparency,
    'increaseContrast': increaseContrast,
    'reduceMotion': reduceMotion,
    'differentiateWithoutColor': differentiateWithoutColor,
    'remoteSession': remoteSession,
    'batterySaver': batterySaver,
  };

  @override
  bool operator ==(Object other) =>
      other is OsAccessibilitySignals &&
      other.reduceTransparency == reduceTransparency &&
      other.increaseContrast == increaseContrast &&
      other.reduceMotion == reduceMotion &&
      other.differentiateWithoutColor == differentiateWithoutColor &&
      other.remoteSession == remoteSession &&
      other.batterySaver == batterySaver;

  @override
  int get hashCode => Object.hash(
    reduceTransparency,
    increaseContrast,
    reduceMotion,
    differentiateWithoutColor,
    remoteSession,
    batterySaver,
  );

  @override
  String toString() => 'OsAccessibilitySignals(${toMap().entries.where((e) => e.value).map((e) => e.key).join(', ')})';
}

/// Why the solid tier is in effect (for Settings / the gallery overlay).
enum GlassSolidReason { reduceTransparency, setting, remoteSession, batterySaver, performance }

/// Result of [resolveEffectiveGlass].
@immutable
final class EffectiveGlass {
  const EffectiveGlass({required this.appearance, this.solidReason});

  final GlassAppearance appearance;

  /// Non-null when [appearance] is in the solid tier.
  final GlassSolidReason? solidReason;

  @override
  bool operator ==(Object other) =>
      other is EffectiveGlass && other.appearance == appearance && other.solidReason == solidReason;

  @override
  int get hashCode => Object.hash(appearance, solidReason);

  @override
  String toString() => 'EffectiveGlass($appearance${solidReason == null ? '' : ', solid: ${solidReason!.name}'})';
}

/// Merges the user's [mode] with OS signals, `MediaQuery` flags and the
/// frame guard into one [GlassAppearance] (LIQUID_GLASS_SPEC §2.10):
///
/// * Reduce Transparency or Glass = Solid → solid tier (tint 1.0, no blur);
/// * Remote Desktop / battery saver → solid tier;
/// * Increase Contrast (OS or `MediaQuery.highContrast`) → contrast tokens;
/// * Reduce Motion (OS or `MediaQuery.disableAnimations`) → fades only and
///   no refraction;
/// * refractive tier only on macOS with shader filters (Impeller).
///
/// [performanceCap] is the highest tier the frame guard still allows.
EffectiveGlass resolveEffectiveGlass({
  required GlassMode mode,
  OsAccessibilitySignals os = OsAccessibilitySignals.none,
  bool mediaHighContrast = false,
  bool mediaDisableAnimations = false,
  TargetPlatform platform = TargetPlatform.macOS,
  bool shaderFilterSupported = false,
  GlassTier performanceCap = GlassTier.refractive,
}) {
  final increaseContrast = os.increaseContrast || mediaHighContrast;
  final reduceMotion = os.reduceMotion || mediaDisableAnimations;

  final GlassSolidReason? solidReason = os.reduceTransparency
      ? GlassSolidReason.reduceTransparency
      : mode == GlassMode.solid
      ? GlassSolidReason.setting
      : os.remoteSession
      ? GlassSolidReason.remoteSession
      : os.batterySaver
      ? GlassSolidReason.batterySaver
      : performanceCap == GlassTier.solid
      ? GlassSolidReason.performance
      : null;

  final GlassTier tier;
  if (solidReason != null) {
    tier = GlassTier.solid;
  } else if (platform == TargetPlatform.macOS &&
      shaderFilterSupported &&
      !reduceMotion &&
      performanceCap == GlassTier.refractive) {
    tier = GlassTier.refractive;
  } else {
    tier = GlassTier.frosted;
  }

  return EffectiveGlass(
    appearance: GlassAppearance(
      mode: mode,
      tier: tier,
      increaseContrast: increaseContrast,
      reduceMotion: reduceMotion,
      reduceTransparency: os.reduceTransparency,
    ),
    solidReason: solidReason,
  );
}
