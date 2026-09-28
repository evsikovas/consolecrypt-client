import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';

/// Loads the refraction shader (`shaders/liquid_glass.frag`) once.
abstract final class GlassShaderProgram {
  static const asset = 'shaders/liquid_glass.frag';

  /// Float uniform count after the engine-bound `u_size` (see the shader).
  static const _uRect = 2;
  static const _uRadius = 6;
  static const _uBezel = 7;
  static const _uDisplacement = 8;
  static const _uChroma = 9;
  static const _uSpecular = 10;

  static Future<ui.FragmentProgram?>? _loading;

  /// Whether refraction can run here at all: macOS + Impeller (§6.5).
  static bool get platformSupported =>
      !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS && ui.ImageFilter.isShaderFilterSupported;

  /// The compiled program, or `null` when unsupported or it failed to load
  /// (the kit then renders frosted glass).
  static Future<ui.FragmentProgram?> load() {
    if (!platformSupported) return Future.value();
    return _loading ??= _load();
  }

  static Future<ui.FragmentProgram?> _load() async {
    try {
      return await ui.FragmentProgram.fromAsset(asset);
    } catch (_) {
      return null;
    }
  }
}

/// Rec.709 saturation matrix for `ColorFilter.matrix` (§2.1).
List<double> glassSaturationMatrix(double s) {
  const r = 0.2126;
  const g = 0.7152;
  const b = 0.0722;
  final i = 1 - s;
  return [
    r * i + s, g * i, b * i, 0, 0, //
    r * i, g * i + s, b * i, 0, 0, //
    r * i, g * i, b * i + s, 0, 0, //
    0, 0, 0, 1, 0, //
  ];
}

/// The live backdrop of a glass surface: blur σ + saturation, and on the
/// refractive tier the edge-refraction shader on top
/// (`ImageFilter.compose(outer: shader, inner: saturation ∘ blur)`).
///
/// It is a [RenderBackdropFilter], so structural checks that look for
/// backdrop filters find it. [grouped] shares the nearest [BackdropGroup]
/// key (persistent, non-overlapping chrome); overlays use their own layer.
class GlassBackdrop extends SingleChildRenderObjectWidget {
  const GlassBackdrop({
    required this.sigma,
    required this.saturation,
    super.key,
    super.child,
    this.refraction = GlassRefraction.none,
    this.program,
    this.cornerRadius = 0,
    this.devicePixelRatio = 1,
    this.grouped = false,
    this.enabled = true,
    this.onRefractionChanged,
  });

  /// `false` paints the child without any backdrop layer (keeps the widget
  /// tree stable when a surface drops to static/solid).
  final bool enabled;
  final double sigma;
  final double saturation;
  final GlassRefraction refraction;

  /// Refraction shader program; `null` = frosted only.
  final ui.FragmentProgram? program;
  final double cornerRadius;
  final double devicePixelRatio;
  final bool grouped;

  /// Reports whether the shader is actually in use (for the budget overlay).
  final ValueChanged<bool>? onRefractionChanged;

  @override
  RenderGlassBackdrop createRenderObject(BuildContext context) => RenderGlassBackdrop(
    enabled: enabled,
    sigma: sigma,
    saturation: saturation,
    refraction: refraction,
    program: program,
    cornerRadius: cornerRadius,
    devicePixelRatio: devicePixelRatio,
    backdropKey: grouped ? BackdropGroup.of(context)?.backdropKey : null,
    onRefractionChanged: onRefractionChanged,
  );

  @override
  void updateRenderObject(BuildContext context, RenderGlassBackdrop renderObject) {
    renderObject
      ..enabled = enabled
      ..sigma = sigma
      ..saturation = saturation
      ..refraction = refraction
      ..program = program
      ..cornerRadius = cornerRadius
      ..devicePixelRatio = devicePixelRatio
      ..backdropKey = grouped ? BackdropGroup.of(context)?.backdropKey : null
      ..onRefractionChanged = onRefractionChanged;
  }
}

/// Render object of [GlassBackdrop].
class RenderGlassBackdrop extends RenderBackdropFilter {
  RenderGlassBackdrop({
    required this._sigma,
    required this._saturation,
    required this._refraction,
    required this._program,
    required this._cornerRadius,
    required this._devicePixelRatio,
    super.enabled,
    super.backdropKey,
    this.onRefractionChanged,
  }) : super(filterConfig: const ImageFilterConfig.blur()) {
    filterConfig = _GlassFilterConfig(this);
  }

  ValueChanged<bool>? onRefractionChanged;
  bool _refracting = false;
  ui.FragmentShader? _shader;

  @override
  bool get alwaysNeedsCompositing => enabled && child != null;

  @override
  set enabled(bool value) {
    if (value == enabled) return;
    super.enabled = value;
    markNeedsCompositingBitsUpdate();
    if (!value) _setRefracting(false);
  }

  @override
  void paint(PaintingContext context, Offset offset) {
    if (!enabled) layer = null;
    super.paint(context, offset);
  }

  void _setRefracting(bool refracting) {
    if (refracting == _refracting) return;
    _refracting = refracting;
    final callback = onRefractionChanged;
    if (callback != null) SchedulerBinding.instance.addPostFrameCallback((_) => callback(refracting));
  }

  double get sigma => _sigma;
  double _sigma;
  set sigma(double value) {
    if (value == _sigma) return;
    _sigma = value;
    markNeedsPaint();
  }

  double get saturation => _saturation;
  double _saturation;
  set saturation(double value) {
    if (value == _saturation) return;
    _saturation = value;
    markNeedsPaint();
  }

  GlassRefraction get refraction => _refraction;
  GlassRefraction _refraction;
  set refraction(GlassRefraction value) {
    if (value == _refraction) return;
    _refraction = value;
    markNeedsPaint();
  }

  ui.FragmentProgram? get program => _program;
  ui.FragmentProgram? _program;
  set program(ui.FragmentProgram? value) {
    if (identical(value, _program)) return;
    _program = value;
    _shader?.dispose();
    _shader = null;
    markNeedsPaint();
  }

  double get cornerRadius => _cornerRadius;
  double _cornerRadius;
  set cornerRadius(double value) {
    if (value == _cornerRadius) return;
    _cornerRadius = value;
    markNeedsPaint();
  }

  double get devicePixelRatio => _devicePixelRatio;
  double _devicePixelRatio;
  set devicePixelRatio(double value) {
    if (value == _devicePixelRatio) return;
    _devicePixelRatio = value;
    markNeedsPaint();
  }

  /// Whether the last paint used the refraction shader.
  bool get isRefracting => _refracting;

  ui.ImageFilter _buildFilter() {
    ui.ImageFilter filter = ui.ImageFilter.blur(sigmaX: _sigma, sigmaY: _sigma, tileMode: ui.TileMode.clamp);
    if (_saturation != 1) {
      filter = ui.ImageFilter.compose(outer: ui.ColorFilter.matrix(glassSaturationMatrix(_saturation)), inner: filter);
    }
    var refracting = false;
    final program = _program;
    if (program != null && !_refraction.isNone && ui.ImageFilter.isShaderFilterSupported && attached) {
      try {
        final shader = _shader ??= program.fragmentShader();
        final dpr = _devicePixelRatio;
        final rect = MatrixUtils.transformRect(getTransformTo(null), Offset.zero & size);
        final radius = _cornerRadius.clamp(0.0, size.shortestSide / 2);
        shader
          ..setFloat(GlassShaderProgram._uRect, rect.left * dpr)
          ..setFloat(GlassShaderProgram._uRect + 1, rect.top * dpr)
          ..setFloat(GlassShaderProgram._uRect + 2, rect.width * dpr)
          ..setFloat(GlassShaderProgram._uRect + 3, rect.height * dpr)
          ..setFloat(GlassShaderProgram._uRadius, radius * dpr)
          ..setFloat(GlassShaderProgram._uBezel, _refraction.bezel * dpr)
          ..setFloat(GlassShaderProgram._uDisplacement, _refraction.maxDisplacement * dpr)
          ..setFloat(GlassShaderProgram._uChroma, _refraction.chroma * dpr)
          ..setFloat(GlassShaderProgram._uSpecular, 0.10);
        filter = ui.ImageFilter.compose(outer: ui.ImageFilter.shader(shader), inner: filter);
        refracting = true;
      } catch (_) {
        // Unsupported backend or invalid program: stay frosted.
        refracting = false;
      }
    }
    _setRefracting(refracting);
    return filter;
  }

  @override
  void dispose() {
    _shader?.dispose();
    _shader = null;
    super.dispose();
  }
}

/// Filter config that resolves at paint time so the shader knows where the
/// surface is on screen.
final class _GlassFilterConfig implements ImageFilterConfig {
  _GlassFilterConfig(this._owner);

  final RenderGlassBackdrop _owner;

  @override
  ui.ImageFilter resolve(ImageFilterContext context) => _owner._buildFilter();

  @override
  ui.ImageFilter? get filter => null;

  @override
  String get debugShortDescription =>
      'glass(σ ${_owner.sigma.toStringAsFixed(1)}, sat ${_owner.saturation}, '
      'refraction ${_owner.refraction.isNone ? 'off' : 'on'})';

  @override
  String toString() => 'ImageFilterConfig.$debugShortDescription';
}
