import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:consolecrypt/core/glass/secure_surface.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Presents a dialog built by [builder] (normally a [GlassDialog])
/// (LIQUID_GLASS_SPEC §4.7).
///
/// * [GlassVariant.thick] (default): live backdrop in its own layer
///   (refractive on macOS), barrier black α .25 / .45, materializes by
///   ramping σ/tint/scale — never by fading a `BackdropFilter`.
/// * [GlassVariant.secure]: `SecureSurface`, no `BackdropFilter`, barrier
///   α .45 / .60; enters with opacity + scale 0.98 over 180 ms (a fade only
///   under Reduce Motion). [opaque] (device approval): α 1.0, barrier
///   α .50 / .65 and no live blur anywhere while open.
/// * [sheet]: macOS attaches it 12 below the 52-pt toolbar band and slides
///   it down 16 px (`spring.smooth`); Windows shows a centred dialog.
Future<T?> showGlassDialog<T>(
  BuildContext context, {
  required WidgetBuilder builder,
  GlassVariant variant = GlassVariant.thick,
  bool opaque = false,
  bool sheet = false,
  bool barrierDismissible = true,
}) {
  assert(variant == GlassVariant.thick || variant == GlassVariant.secure || variant == GlassVariant.regular);
  final navigator = Navigator.of(context, rootNavigator: true);
  final tokens = GlassTokens.of(context);
  final secure = variant == GlassVariant.secure;
  return navigator.push(
    _GlassDialogRoute<T>(
      builder: builder,
      variant: variant,
      approval: opaque && secure,
      sheet: sheet && tokens.platform == TargetPlatform.macOS,
      barrierDismissible: barrierDismissible,
      barrierColor: !secure
          ? tokens.surfaces.barrier
          : (opaque ? tokens.surfaces.approvalBarrier : tokens.surfaces.secureBarrier),
      themes: InheritedTheme.capture(from: context, to: navigator.context),
      scope: GlassScope.of(context),
      barrierLabel: GlassStrings.of(context).dismiss,
    ),
  );
}

/// Route state handed to [GlassDialog] (material + entrance animation).
class GlassDialogScope extends InheritedWidget {
  const GlassDialogScope({
    required this.variant,
    required this.animation,
    required this.opaque,
    required this.sheet,
    required super.child,
    super.key,
  });

  final GlassVariant variant;
  final Animation<double> animation;
  final bool opaque;
  final bool sheet;

  static GlassDialogScope? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<GlassDialogScope>();

  @override
  bool updateShouldNotify(GlassDialogScope oldWidget) =>
      variant != oldWidget.variant || animation != oldWidget.animation || opaque != oldWidget.opaque;
}

class _GlassDialogRoute<T> extends PopupRoute<T> {
  _GlassDialogRoute({
    required this.builder,
    required this.variant,
    required this.approval,
    required this.sheet,
    required this._barrierDismissible,
    required this._barrierColor,
    required this.themes,
    required this.scope,
    required this.barrierLabel,
  });

  final WidgetBuilder builder;
  final GlassVariant variant;

  /// Device approval: secure at α 1.0 (not `opaque`, which is the route's
  /// "hide everything below" flag).
  final bool approval;
  final bool sheet;
  final CapturedThemes themes;
  final GlassScopeData scope;
  final bool _barrierDismissible;
  final Color _barrierColor;

  bool get _secure => variant == GlassVariant.secure;

  @override
  bool get barrierDismissible => _barrierDismissible;

  @override
  Color get barrierColor => _barrierColor;

  @override
  final String barrierLabel;

  @override
  Duration get transitionDuration => _secure ? GlassMotion.base : GlassMotion.slow;

  @override
  Duration get reverseTransitionDuration => _secure ? GlassMotion.fast : GlassMotion.dematerialize;

  @override
  Widget buildPage(BuildContext context, Animation<double> animation, Animation<double> secondaryAnimation) {
    final page = GlassDialogScope(
      variant: variant,
      animation: animation,
      opaque: approval,
      sheet: sheet,
      child: Builder(builder: builder),
    );
    final Widget positioned = sheet
        ? Align(
            alignment: Alignment.topCenter,
            child: Padding(
              padding: const EdgeInsets.only(top: GlassSizes.toolbarBand + 12),
              child: page,
            ),
          )
        : Center(child: page);
    return themes.wrap(
      GlassScope(
        data: scope,
        child: SafeArea(
          child: Padding(padding: const EdgeInsets.all(GlassSpacing.s24), child: positioned),
        ),
      ),
    );
  }

  @override
  Widget buildTransitions(
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) {
    final reduceMotion = scope.appearance.reduceMotion;
    if (_secure) {
      // No BackdropFilter inside the secure material, so a fade is allowed.
      final curved = CurvedAnimation(parent: animation, curve: GlassMotion.appear, reverseCurve: GlassMotion.dismiss);
      Widget result = FadeTransition(opacity: curved, child: child);
      if (!reduceMotion) {
        result = ScaleTransition(scale: Tween(begin: 0.98, end: 1.0).animate(curved), child: result);
      }
      return result;
    }
    // Glass: the surface ramps σ/tint itself (presence); only movement here.
    if (reduceMotion) return child;
    return AnimatedBuilder(
      animation: animation,
      child: child,
      builder: (context, child) {
        if (animation.status == AnimationStatus.reverse) return child!;
        if (sheet) {
          final t = GlassMotion.springCurve(GlassMotion.springSmooth, GlassMotion.slow).transform(animation.value);
          return Transform.translate(offset: Offset(0, -16 * (1 - t)), child: child);
        }
        final t = GlassMotion.springCurve(GlassMotion.springSnappy, GlassMotion.medium).transform(animation.value);
        final scale = GlassMotion.materializeScale + (1 - GlassMotion.materializeScale) * t;
        return Transform.translate(
          offset: Offset(0, GlassMotion.materializeOffset * (1 - t)),
          child: Transform.scale(scale: scale, child: child),
        );
      },
    );
  }
}

class _SubmitIntent extends Intent {
  const _SubmitIntent();
}

class _SubmitAction extends Action<_SubmitIntent> {
  _SubmitAction(this.onSubmit);

  final VoidCallback onSubmit;

  static bool _focusInMultilineField() {
    final focusContext = FocusManager.instance.primaryFocus?.context;
    if (focusContext == null) return false;
    final editable = focusContext.findAncestorWidgetOfExactType<EditableText>();
    return editable != null && editable.maxLines != 1;
  }

  @override
  bool isEnabled(_SubmitIntent intent) => !_focusInMultilineField();

  @override
  Object? invoke(_SubmitIntent intent) {
    onSubmit();
    return null;
  }
}

/// Dialog layout (§4.7): optional leading icon (28, role colour), title2
/// title left-aligned, content, and a trailing action row in platform order
/// (macOS: primary rightmost; Windows: primary first). Padding 24 (28 on the
/// approval dialog). Uses the material chosen by [showGlassDialog]; outside
/// a glass route it renders as a static `glass.thick` panel.
///
/// [onSubmit]: Enter triggers it unless focus is in a multi-line field.
/// Escape = Cancel (the route). Give the least destructive action
/// `autofocus: true`.
class GlassDialog extends StatelessWidget {
  const GlassDialog({
    super.key,
    this.title,
    this.icon,
    this.iconTone = GlassTone.neutral,
    this.content,
    this.primaryAction,
    this.secondaryActions = const [],
    this.leadingAction,
    this.width = 480,
    this.onSubmit,
  });

  final String? title;
  final IconData? icon;
  final GlassTone iconTone;
  final Widget? content;
  final Widget? primaryAction;
  final List<Widget> secondaryActions;

  /// Left-aligned action (e.g. "Codes don't match").
  final Widget? leadingAction;

  /// 480 or 560 (600 for device approval).
  final double width;
  final VoidCallback? onSubmit;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final route = GlassDialogScope.maybeOf(context);
    final variant = route?.variant ?? GlassVariant.thick;
    final secure = variant == GlassVariant.secure;
    final opaque = route?.opaque ?? false;
    final mac = tokens.platform == TargetPlatform.macOS;

    final actions = <Widget>[if (mac) ...secondaryActions, ?primaryAction, if (!mac) ...secondaryActions];
    final column = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (title != null || icon != null) ...[
          Row(
            children: [
              if (icon != null) ...[
                Icon(icon, size: 28, color: tokens.palette.tone(iconTone)),
                const SizedBox(width: GlassSpacing.s12),
              ],
              if (title != null)
                Expanded(
                  child: Semantics(
                    header: true,
                    child: Text(title!, style: tokens.typography.title2.copyWith(color: tokens.palette.label)),
                  ),
                ),
            ],
          ),
          const SizedBox(height: GlassSpacing.s16),
        ],
        if (content != null)
          Flexible(
            child: DefaultTextStyle.merge(
              style: tokens.typography.body.copyWith(color: tokens.palette.label),
              child: content!,
            ),
          ),
        if (actions.isNotEmpty || leadingAction != null) ...[
          const SizedBox(height: GlassSpacing.s24),
          Row(
            children: [
              if (leadingAction != null) ...[Flexible(child: leadingAction!), const SizedBox(width: GlassSpacing.s12)],
              Expanded(
                child: OverflowBar(
                  alignment: MainAxisAlignment.end,
                  overflowAlignment: OverflowBarAlignment.end,
                  spacing: GlassSpacing.s8,
                  overflowSpacing: GlassSpacing.s8,
                  children: actions,
                ),
              ),
            ],
          ),
        ],
      ],
    );
    // material_ui controls (checkboxes, text fields) need a Material ancestor.
    final body = Material(type: MaterialType.transparency, child: column);

    final padding = EdgeInsets.all(opaque ? GlassSpacing.secureDialog : GlassSpacing.dialog);
    Widget surface;
    if (secure) {
      surface = SecureSurface(padding: padding, opaque: opaque, suppressLiveBlur: opaque, child: body);
    } else {
      final animation = route?.animation;
      surface = animation == null
          ? GlassSurface(variant: variant, shape: GlassRadii.shape(tokens.radii.dialog), padding: padding, child: body)
          : AnimatedBuilder(
              animation: animation,
              builder: (context, child) {
                final reverse = animation.status == AnimationStatus.reverse;
                final presence = (reverse ? GlassMotion.dismiss : GlassMotion.appear).transform(animation.value);
                return GlassSurface(
                  variant: variant,
                  shape: GlassRadii.shape(tokens.radii.dialog),
                  padding: padding,
                  backdrop: BackdropMode.live,
                  overlay: true,
                  presence: presence,
                  child: child!,
                );
              },
              child: body,
            );
    }

    Widget result = ConstrainedBox(
      constraints: BoxConstraints(maxWidth: width),
      child: Semantics(scopesRoute: true, namesRoute: true, explicitChildNodes: true, label: title, child: surface),
    );
    final submit = onSubmit;
    if (submit != null) {
      result = Shortcuts(
        shortcuts: const {
          SingleActivator(LogicalKeyboardKey.enter): _SubmitIntent(),
          SingleActivator(LogicalKeyboardKey.numpadEnter): _SubmitIntent(),
        },
        child: Actions(actions: {_SubmitIntent: _SubmitAction(submit)}, child: result),
      );
    }
    return result;
  }
}

/// Debug check used by tests: whether [route] is a glass dialog route.
@visibleForTesting
bool isGlassDialogRoute(Route<dynamic> route) => route is _GlassDialogRoute;
