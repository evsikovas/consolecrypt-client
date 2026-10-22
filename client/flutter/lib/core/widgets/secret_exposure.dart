import 'package:consolecrypt/core/widgets/obscurable_text_controller.dart';
import 'package:flutter/widgets.dart';

/// Counts the secrets currently shown in plain text (a revealed password
/// field, recovery words, unmasked environment values, …) so the window can
/// be hidden from screen capture while any is on screen.
///
/// Secret-revealing widgets do not talk to this directly: they wrap the
/// revealed content in [RevealedSecret], which holds a count while it is
/// mounted and visible.
class SecretExposure extends ChangeNotifier {
  SecretExposure();

  /// The app-wide counter.
  static final instance = SecretExposure();

  int _count = 0;

  /// Number of secrets currently revealed.
  int get count => _count;

  /// Whether any secret is revealed.
  bool get visible => _count > 0;

  /// Registers one revealed secret. Call the returned function to release
  /// it; extra calls are ignored.
  VoidCallback hold() {
    var released = false;
    _count++;
    if (_count == 1) notifyListeners();
    return () {
      if (released) return;
      released = true;
      _count--;
      if (_count == 0) notifyListeners();
    };
  }
}

/// Marks [child] as showing a secret in plain text while [visible].
class RevealedSecret extends StatefulWidget {
  const RevealedSecret({super.key, this.visible = true, required this.child, this.exposure});

  /// Whether the secret is currently shown unmasked.
  final bool visible;

  final Widget child;

  /// Counter to report to; [SecretExposure.instance] by default.
  final SecretExposure? exposure;

  @override
  State<RevealedSecret> createState() => _RevealedSecretState();
}

class _RevealedSecretState extends State<RevealedSecret> {
  VoidCallback? _release;

  SecretExposure get _exposure => widget.exposure ?? SecretExposure.instance;

  @override
  void initState() {
    super.initState();
    _sync();
  }

  @override
  void didUpdateWidget(RevealedSecret oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.exposure != widget.exposure) {
      _release?.call();
      _release = null;
    }
    _sync();
  }

  void _sync() {
    if (widget.visible) {
      _release ??= _exposure.hold();
    } else {
      _release?.call();
      _release = null;
    }
  }

  @override
  void dispose() {
    _release?.call();
    _release = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// A secret typed in plain text (a pasted private key, a multi-line secret):
/// counts as revealed while [enabled] and [controller] holds text that is
/// not drawn as bullets ([ObscurableTextController.obscured]).
class RevealedSecretText extends StatelessWidget {
  const RevealedSecretText({super.key, required this.controller, this.enabled = true, required this.child});

  final TextEditingController controller;
  final bool enabled;
  final Widget child;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: controller,
    builder: (context, child) => RevealedSecret(
      visible:
          enabled &&
          controller.text.isNotEmpty &&
          !(controller is ObscurableTextController && (controller as ObscurableTextController).obscured),
      child: child!,
    ),
    child: child,
  );
}
