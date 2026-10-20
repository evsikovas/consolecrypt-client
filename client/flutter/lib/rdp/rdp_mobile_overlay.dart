import 'dart:async';
import 'dart:math' as math;
import 'dart:ui';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:material_ui/material_ui.dart';

class RdpMobileAction {
  const RdpMobileAction(this.label, this.icon, this.invoke, {this.key, this.selected = false});
  final String label;
  final IconData icon;
  final VoidCallback? invoke;
  final Key? key;
  final bool selected;
}

class RdpMobileOverlay extends StatefulWidget {
  const RdpMobileOverlay({
    super.key,
    required this.actions,
    required this.open,
    required this.onOpenChanged,
    required this.position,
    required this.onPositionChanged,
    required this.touching,
    required this.autoHide,
    this.bottomInset = 0,
  });
  final List<RdpMobileAction> actions;
  final bool open, touching, autoHide;
  final ValueChanged<bool> onOpenChanged;
  final List<double> position;
  final ValueChanged<List<double>> onPositionChanged;
  final double bottomInset;

  @override
  State<RdpMobileOverlay> createState() => _RdpMobileOverlayState();
}

class _RdpMobileOverlayState extends State<RdpMobileOverlay> {
  Timer? _timer;
  bool _dim = false;
  Offset? _drag;

  bool get _accessible => MediaQuery.accessibleNavigationOf(context);

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _schedule();
  }

  @override
  void didUpdateWidget(RdpMobileOverlay oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.open != widget.open ||
        oldWidget.touching != widget.touching ||
        oldWidget.autoHide != widget.autoHide) {
      _dim = false;
      _schedule();
    }
  }

  void _schedule() {
    _timer?.cancel();
    if (_accessible || !widget.autoHide || widget.touching) return;
    _timer = Timer(Duration(seconds: widget.open ? 5 : 3), () {
      if (!mounted) return;
      if (widget.open) {
        widget.onOpenChanged(false);
      } else {
        setState(() => _dim = true);
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final extent = Size(
        math.max(0, constraints.maxWidth - 44),
        math.max(0, constraints.maxHeight - widget.bottomInset - 44),
      );
      final point = _drag ?? Offset(widget.position[0] * extent.width, widget.position[1] * extent.height);
      final position = Offset(point.dx.clamp(0, extent.width), point.dy.clamp(0, extent.height));
      final hidden = widget.touching && !widget.open && widget.autoHide && !_accessible && _drag == null;
      return Stack(
        children: [
          if (widget.open)
            Positioned.fill(
              child: GestureDetector(
                key: const ValueKey('rdp-menu-dismiss'),
                behavior: HitTestBehavior.opaque,
                onTap: () => widget.onOpenChanged(false),
              ),
            ),
          if (widget.open)
            // A card inside the screen margins: actions wrap into rows
            // instead of running off both edges, and the card sits above
            // the menu button in the lower half, below it in the upper half.
            Builder(
              builder: (context) {
                final above = position.dy - 16;
                final below = constraints.maxHeight - widget.bottomInset - position.dy - 60;
                final up = above >= below;
                return Positioned(
                  left: 12,
                  right: 12,
                  top: up ? null : position.dy + 52,
                  bottom: up ? constraints.maxHeight - position.dy + 8 : null,
                  child: Center(
                    child: ConstrainedBox(
                      constraints: BoxConstraints(maxWidth: 820, maxHeight: math.max(64, up ? above : below)),
                      child: _GlassSurface(
                        radius: 20,
                        child: Listener(
                          onPointerDown: (_) => _timer?.cancel(),
                          onPointerUp: (_) => _schedule(),
                          child: LayoutBuilder(
                            builder: (context, box) {
                              // An even grid of tiles (icon over a short label).
                              const gap = 6.0;
                              final fit = ((box.maxWidth - 16 + gap) / (92 + gap)).floor().clamp(3, 8);
                              final count = math.max(1, widget.actions.length);
                              // Balanced rows: no lone tile on the last row.
                              final columns = math.min(fit, (count / (count / fit).ceil()).ceil());
                              final tile = (box.maxWidth - 16 - gap * (columns - 1)) / columns;
                              return SingleChildScrollView(
                                key: const ValueKey('rdp-mobile-panel'),
                                padding: const EdgeInsets.all(8),
                                child: ExcludeFocus(
                                  child: Wrap(
                                    spacing: gap,
                                    runSpacing: gap,
                                    children: [
                                      for (final action in widget.actions)
                                        SizedBox(
                                          width: tile,
                                          height: 72,
                                          child: TextButton(
                                            key: action.key,
                                            style: TextButton.styleFrom(
                                              padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 6),
                                              backgroundColor: action.selected
                                                  ? const Color(0x3382b4ff)
                                                  : const Color(0x14ffffff),
                                              foregroundColor: action.selected ? const Color(0xff82b4ff) : Colors.white,
                                              shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
                                            ),
                                            onPressed: action.invoke == null
                                                ? null
                                                : () {
                                                    _schedule();
                                                    action.invoke!();
                                                  },
                                            child: Column(
                                              mainAxisAlignment: MainAxisAlignment.center,
                                              children: [
                                                Icon(action.icon, size: 22),
                                                const SizedBox(height: 4),
                                                Text(
                                                  action.label,
                                                  maxLines: 2,
                                                  overflow: TextOverflow.ellipsis,
                                                  textAlign: TextAlign.center,
                                                  style: const TextStyle(fontSize: 11.5, height: 1.15),
                                                ),
                                              ],
                                            ),
                                          ),
                                        ),
                                    ],
                                  ),
                                ),
                              );
                            },
                          ),
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          Positioned(
            left: position.dx,
            top: position.dy,
            child: IgnorePointer(
              ignoring: hidden,
              child: AnimatedOpacity(
                duration: MediaQuery.disableAnimationsOf(context) ? Duration.zero : const Duration(milliseconds: 150),
                opacity: hidden ? 0 : (_dim && !widget.open && !_accessible ? .3 : 1),
                child: GestureDetector(
                  onPanStart: (_) {
                    _timer?.cancel();
                    setState(() {
                      _drag = position;
                      _dim = false;
                    });
                  },
                  onPanUpdate: (event) => setState(() => _drag = (_drag ?? position) + event.delta),
                  onPanEnd: (_) {
                    final next = [
                      position.dx < extent.width / 2 ? 0.0 : 1.0,
                      extent.height == 0 ? .5 : position.dy / extent.height,
                    ];
                    setState(() => _drag = null);
                    widget.onPositionChanged(next);
                    _schedule();
                  },
                  child: _GlassSurface(
                    radius: 22,
                    child: SizedBox.square(
                      dimension: 44,
                      child: ExcludeFocus(
                        child: IconButton(
                          key: const ValueKey('rdp-mobile-menu'),
                          tooltip: context.l10n.rdpMobileMenu,
                          constraints: const BoxConstraints.tightFor(width: 44, height: 44),
                          padding: EdgeInsets.zero,
                          color: Colors.white,
                          onPressed: () {
                            setState(() => _dim = false);
                            widget.onOpenChanged(!widget.open);
                          },
                          icon: Icon(widget.open ? Icons.close : Icons.adjust),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      );
    },
  );
}

class _GlassSurface extends StatelessWidget {
  const _GlassSurface({required this.child, this.radius = 16});
  final Widget child;
  final double radius;
  @override
  Widget build(BuildContext context) => ClipRRect(
    borderRadius: BorderRadius.circular(radius),
    child: BackdropFilter(
      filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
      child: Material(color: const Color(0xb317243b), child: child),
    ),
  );
}

/// Sticky modifiers are sent through the same ordered session queue as keys.
/// Cancelling only clears local state; the owning view issues native ReleaseAll.
class RdpMobileKeys extends ChangeNotifier {
  RdpMobileKeys(this.send);
  final ValueChanged<List<RdpInput>> send;
  final Map<int, bool> _modifiers = {};
  final Map<int, DateTime> _taps = {};
  static const codes = [0x1d, 0x38, 0x5b, 0x2a];

  bool active(int code) => _modifiers.containsKey(code);
  bool locked(int code) => _modifiers[code] == true;
  bool get hasShortcutModifier => active(0x1d) || active(0x38) || active(0x5b);

  void modifier(int code, {DateTime? at}) {
    final now = at ?? DateTime.now();
    final previous = _taps[code];
    if (active(code) && !locked(code) && previous != null && now.difference(previous).inMilliseconds <= 350) {
      _modifiers[code] = true;
    } else if (active(code)) {
      _modifiers.remove(code);
      send([RdpScancodeInput(code, extended: code == 0x5b, down: false)]);
    } else {
      _modifiers[code] = false;
      send([RdpScancodeInput(code, extended: code == 0x5b, down: true)]);
    }
    _taps[code] = now;
    notifyListeners();
  }

  void key(int code, {bool extended = false}) {
    send([
      RdpScancodeInput(code, extended: extended, down: true),
      RdpScancodeInput(code, extended: extended, down: false),
    ]);
    consume();
  }

  void consume() {
    final released = _modifiers.entries.where((entry) => !entry.value).map((e) => e.key).toList();
    if (released.isEmpty) return;
    send([for (final code in released) RdpScancodeInput(code, extended: code == 0x5b, down: false)]);
    for (final code in released) {
      _modifiers.remove(code);
      _taps.remove(code);
    }
    notifyListeners();
  }

  void cancel() {
    _modifiers.clear();
    _taps.clear();
  }
}
