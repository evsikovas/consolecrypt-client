import 'dart:async';

import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_budget.dart';
import 'package:consolecrypt/core/glass/glass_button.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:flutter/semantics.dart';
import 'package:material_ui/material_ui.dart';

/// A request to show a toast (see [GlassToast.show]).
@immutable
final class GlassToastRequest {
  const GlassToastRequest({
    required this.message,
    this.tone = GlassTone.neutral,
    this.icon,
    this.actionLabel,
    this.onAction,
    this.duration,
    this.countdown,
  });

  final String message;
  final GlassTone tone;
  final IconData? icon;
  final String? actionLabel;
  final VoidCallback? onAction;

  /// `null` → 4 s, except [GlassTone.danger] which stays until dismissed.
  final Duration? duration;

  /// Clipboard toasts: countdown ring + "Clears in N s". Never show the
  /// copied value in [message].
  final Duration? countdown;

  bool get persistent => duration == null && countdown == null && tone == GlassTone.danger;

  Duration get effectiveDuration => countdown ?? duration ?? const Duration(seconds: 4);
}

/// Handle of a shown toast.
final class GlassToastHandle {
  GlassToastHandle._(this._host, this._id);

  final _ToastHostController _host;
  final int _id;

  void dismiss() => _host.dismiss(_id);
}

/// Toasts (LIQUID_GLASS_SPEC §4.11): `glass.regular` capsule, height 44,
/// width 320–520, bottom centre 24 above the window bottom; stacks at most
/// 3 with 8 px gaps. Materializes from below (a fade under Reduce Motion),
/// announced to screen readers (assertive for errors). On routes that
/// suppress live blur (terminal: no glass over the viewport without a
/// barrier) toasts render in the solid tier.
abstract final class GlassToast {
  static final Expando<_ToastHostController> _hosts = Expando('glass-toast-host');

  /// Shows [request] on the root overlay of [context].
  static GlassToastHandle show(BuildContext context, GlassToastRequest request) {
    final overlay = Overlay.of(context, rootOverlay: true);
    final host = _hosts[overlay] ??= _ToastHostController(overlay);
    return host.add(
      _ToastItem(
        id: _ToastHostController._nextId++,
        request: request,
        scope: GlassScope.of(context),
        themes: InheritedTheme.capture(from: context, to: overlay.context),
      ),
    );
  }
}

final class _ToastItem {
  _ToastItem({required this.id, required this.request, required this.scope, required this.themes});

  final int id;
  final GlassToastRequest request;
  final GlassScopeData scope;
  final CapturedThemes themes;
  bool leaving = false;
}

/// Owns the overlay entry and the visible toasts of one overlay.
final class _ToastHostController extends ChangeNotifier {
  _ToastHostController(this._overlay);

  static int _nextId = 0;
  static const _max = 3;

  final OverlayState _overlay;
  final List<_ToastItem> _items = [];
  OverlayEntry? _entry;

  List<_ToastItem> get items => _items;

  GlassToastHandle add(_ToastItem item) {
    _items.add(item);
    while (_items.length > _max) {
      _items.removeAt(0);
    }
    if (_entry == null && _overlay.mounted) {
      final entry = OverlayEntry(builder: (_) => _ToastHost(controller: this));
      _entry = entry;
      _overlay.insert(entry);
    }
    notifyListeners();
    return GlassToastHandle._(this, item.id);
  }

  void dismiss(int id) {
    for (final item in _items) {
      if (item.id == id && !item.leaving) {
        item.leaving = true;
        notifyListeners();
      }
    }
  }

  void remove(int id) {
    _items.removeWhere((i) => i.id == id);
    if (_items.isEmpty) {
      _entry?.remove();
      _entry = null;
    }
    notifyListeners();
  }
}

class _ToastHost extends StatelessWidget {
  const _ToastHost({required this.controller});

  final _ToastHostController controller;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, _) {
      final items = controller.items;
      if (items.isEmpty) return const SizedBox.shrink();
      return Positioned(
        left: 0,
        right: 0,
        bottom: GlassSpacing.s24,
        child: SafeArea(
          top: false,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              for (final (i, item) in items.indexed) ...[
                if (i > 0) const SizedBox(height: GlassSpacing.s8),
                item.themes.wrap(
                  _GlassToastView(
                    key: ValueKey('glass-toast-${item.id}'),
                    request: item.request,
                    scope: item.scope,
                    leaving: item.leaving,
                    onDismiss: () => controller.dismiss(item.id),
                    onGone: () => controller.remove(item.id),
                  ),
                ),
              ],
            ],
          ),
        ),
      );
    },
  );
}

class _GlassToastView extends StatefulWidget {
  const _GlassToastView({
    required this.request,
    required this.scope,
    required this.leaving,
    required this.onDismiss,
    required this.onGone,
    super.key,
  });

  final GlassToastRequest request;
  final GlassScopeData scope;
  final bool leaving;
  final VoidCallback onDismiss;
  final VoidCallback onGone;

  @override
  State<_GlassToastView> createState() => _GlassToastViewState();
}

class _GlassToastViewState extends State<_GlassToastView> with TickerProviderStateMixin {
  late final AnimationController _presence = AnimationController(
    vsync: this,
    duration: widget.scope.appearance.reduceMotion ? GlassMotion.fast : GlassMotion.medium,
    reverseDuration: GlassMotion.dematerialize,
  );
  AnimationController? _countdown;
  Timer? _timer;
  bool _announced = false;

  @override
  void initState() {
    super.initState();
    unawaited(_presence.forward());
    final request = widget.request;
    if (request.countdown != null) {
      _countdown = AnimationController(vsync: this, duration: request.countdown)..forward();
    }
    if (!request.persistent) _timer = Timer(request.effectiveDuration, widget.onDismiss);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!_announced) {
      _announced = true;
      final view = View.maybeOf(context);
      if (view != null) {
        unawaited(
          SemanticsService.sendAnnouncement(
            view,
            widget.request.message,
            Directionality.maybeOf(context) ?? TextDirection.ltr,
            assertiveness: widget.request.tone == GlassTone.danger ? Assertiveness.assertive : Assertiveness.polite,
          ),
        );
      }
    }
  }

  @override
  void didUpdateWidget(_GlassToastView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.leaving && !oldWidget.leaving) {
      _timer?.cancel();
      unawaited(_presence.reverse().whenComplete(widget.onGone));
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    _presence.dispose();
    _countdown?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final request = widget.request;
    final color = request.tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(request.tone);
    final countdown = _countdown;

    final content = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (countdown != null)
          RepaintBoundary(
            child: SizedBox.square(
              dimension: 18,
              child: AnimatedBuilder(
                animation: countdown,
                builder: (context, _) => CircularProgressIndicator(
                  value: 1 - countdown.value,
                  strokeWidth: 2,
                  color: color,
                  backgroundColor: tokens.surfaces.fillPressed,
                ),
              ),
            ),
          )
        else if (request.icon != null)
          Icon(request.icon, size: 18, color: color),
        const SizedBox(width: GlassSpacing.s8),
        Flexible(
          child: Text(
            request.message,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: tokens.typography.body.copyWith(color: tokens.palette.label),
          ),
        ),
        if (countdown != null) ...[
          const SizedBox(width: GlassSpacing.s8),
          AnimatedBuilder(
            animation: countdown,
            builder: (context, _) {
              final total = request.countdown!.inSeconds;
              final left = (total * (1 - countdown.value)).ceil();
              return Text(
                GlassStrings.of(context).clearsIn(left),
                style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
              );
            },
          ),
        ],
        if (request.actionLabel != null) ...[
          const SizedBox(width: GlassSpacing.s8),
          GlassButton.plain(
            label: request.actionLabel!,
            size: GlassControlSize.sm,
            onPressed: () {
              request.onAction?.call();
              widget.onDismiss();
            },
          ),
        ],
        const SizedBox(width: GlassSpacing.s4),
        GlassIconButton(
          icon: Icons.close_rounded,
          tooltip: GlassStrings.of(context).dismiss,
          style: GlassIconButtonStyle.plain,
          size: 24,
          iconSize: 16,
          onPressed: widget.onDismiss,
        ),
      ],
    );

    return ValueListenableBuilder<GlassSuppression>(
      valueListenable: widget.scope.budget.suppression,
      builder: (context, suppression, child) {
        final scope = suppression == GlassSuppression.none
            ? widget.scope
            : widget.scope.copyWith(appearance: widget.scope.appearance.copyWith(tier: GlassTier.solid));
        return GlassScope(data: scope, child: child!);
      },
      child: AnimatedBuilder(
        animation: _presence,
        builder: (context, child) {
          final reverse = _presence.status == AnimationStatus.reverse;
          final t = (reverse ? GlassMotion.dismiss : GlassMotion.appear).transform(_presence.value);
          final movement = !widget.scope.appearance.reduceMotion && !reverse;
          return Transform.translate(
            offset: Offset(0, movement ? 16 * (1 - t) : 0),
            child: ConstrainedBox(
              constraints: const BoxConstraints(
                minWidth: GlassSizes.toastMinWidth,
                maxWidth: GlassSizes.toastMaxWidth,
                minHeight: GlassSizes.toast,
              ),
              child: GlassSurface(
                variant: GlassVariant.regular,
                shape: const StadiumBorder(),
                backdrop: BackdropMode.live,
                overlay: true,
                presence: t,
                padding: const EdgeInsetsDirectional.only(start: GlassSpacing.s16, end: GlassSpacing.s8),
                child: child!,
              ),
            ),
          );
        },
        child: SizedBox(
          height: GlassSizes.toast,
          child: Semantics(liveRegion: true, child: content),
        ),
      ),
    );
  }
}
