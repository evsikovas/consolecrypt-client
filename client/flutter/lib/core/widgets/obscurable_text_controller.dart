import 'package:material_ui/material_ui.dart';

/// A text controller that draws its text as bullets while [obscured] — for
/// multi-line secrets such as recovery words (`TextField.obscureText` is
/// single-line only).
///
/// While obscured the real characters are never laid out, unlike a blur
/// (partially reversible, and it still renders the secret into GPU
/// textures; LIQUID_GLASS_SPEC §4.14). Each character maps to one bullet, so
/// cursor and selection offsets stay valid.
class ObscurableTextController extends TextEditingController {
  ObscurableTextController({super.text});

  static const bullet = '•';
  static final _visible = RegExp(r'\S');

  bool _obscured = true;

  bool get obscured => _obscured;

  set obscured(bool value) {
    if (value == _obscured) return;
    _obscured = value;
    notifyListeners();
  }

  /// What the field shows for [text] while obscured (whitespace is kept so
  /// the word count stays visible).
  static String mask(String text) => text.replaceAll(_visible, bullet);

  @override
  TextSpan buildTextSpan({required BuildContext context, required bool withComposing, TextStyle? style}) {
    if (!_obscured) return super.buildTextSpan(context: context, style: style, withComposing: withComposing);
    return TextSpan(style: style, text: mask(text));
  }
}
