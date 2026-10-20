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
            Positioned(
              left: 0,
              right: 0,
              top: (position.dy > 60 ? position.dy - 58 : position.dy + 48).clamp(
                0,
                math.max(0, constraints.maxHeight - widget.bottomInset - 48),
              ),
              child: _GlassSurface(
                child: Listener(
                  onPointerDown: (_) => _timer?.cancel(),
                  onPointerUp: (_) => _schedule(),
                  child: SingleChildScrollView(
                    key: const ValueKey('rdp-mobile-panel'),
                    scrollDirection: Axis.horizontal,
                    child: ExcludeFocus(
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          for (final action in widget.actions)
                            TextButton.icon(
                              key: action.key,
                              style: TextButton.styleFrom(
                                minimumSize: const Size(44, 48),
                                foregroundColor: action.selected ? const Color(0xff82b4ff) : Colors.white,
                              ),
                              onPressed: action.invoke == null
                                  ? null
                                  : () {
                                      _schedule();
                                      action.invoke!();
                                    },
                              icon: Icon(action.icon, size: 20),
                              label: Text(action.label),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
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
