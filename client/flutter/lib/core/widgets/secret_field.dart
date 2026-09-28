import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/keyboard_layout.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Obscured single-line field for passwords, passphrases and API keys
/// (LIQUID_GLASS_SPEC §4.4): mono, obscured by default with an eye toggle, a
/// Caps Lock note while focused, a keyboard-layout note when the typed text
/// contains letters outside A–Z (e.g. Cyrillic — the usual cause of a
/// "wrong passphrase"), and no autocorrect/suggestions/IME learning so the
/// OS keyboard never learns the secret. Revealing is an explicit action.
///
/// Secret fields live on content surfaces or the secure material only —
/// never on translucent glass (asserted in debug builds).
class SecretField extends StatefulWidget {
  const SecretField({
    required this.controller,
    required this.label,
    super.key,
    this.hint,
    this.helper,
    this.validator,
    this.onSubmitted,
    this.onChanged,
    this.autofocus = false,
    this.enabled = true,
    this.allowReveal = true,
    this.textInputAction,
    this.autofillHints,
    this.layoutHint = true,
  });

  final TextEditingController controller;
  final String label;
  final String? hint;
  final String? helper;
  final FormFieldValidator<String>? validator;
  final ValueChanged<String>? onSubmitted;
  final ValueChanged<String>? onChanged;
  final bool autofocus;
  final bool enabled;
  final bool allowReveal;
  final TextInputAction? textInputAction;
  final Iterable<String>? autofillHints;

  /// Warn when the typed text contains letters outside A–Z.
  final bool layoutHint;

  @override
  State<SecretField> createState() => _SecretFieldState();
}

class _SecretFieldState extends State<SecretField> {
  final _focus = FocusNode(debugLabel: 'SecretField');
  bool _revealed = false;
  bool _focused = false;
  bool _capsLock = false;
  TypedScript? _script;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocus);
    HardwareKeyboard.instance.addHandler(_onKey);
    _capsLock = HardwareKeyboard.instance.lockModesEnabled.contains(KeyboardLockMode.capsLock);
    widget.controller.addListener(_onText);
    _script = nonLatinScript(widget.controller.text);
  }

  @override
  void didUpdateWidget(SecretField oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(_onText);
      widget.controller.addListener(_onText);
      _script = nonLatinScript(widget.controller.text);
    }
  }

  void _onText() {
    final script = nonLatinScript(widget.controller.text);
    if (script != _script && mounted) setState(() => _script = script);
  }

  void _onFocus() {
    if (mounted) setState(() => _focused = _focus.hasFocus);
  }

  bool _onKey(KeyEvent event) {
    final caps = HardwareKeyboard.instance.lockModesEnabled.contains(KeyboardLockMode.capsLock);
    if (caps != _capsLock && mounted) setState(() => _capsLock = caps);
    return false;
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onText);
    HardwareKeyboard.instance.removeHandler(_onKey);
    _focus
      ..removeListener(_onFocus)
      ..dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    assert(() {
      final onGlass = GlassOnGlass.maybeOf(context);
      if (onGlass != null && !onGlass.secure) {
        throw FlutterError(
          'SecretField must sit on a content surface or the secure material, ' // l10n-ignore: assert message
          'not on ${onGlass.variant.name} glass (LIQUID_GLASS_SPEC §4.4).', // l10n-ignore: assert message
        );
      }
      return true;
    }());
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final notes = [
      if (_focused && _capsLock)
        FieldNote(key: const ValueKey('caps-lock-note'), text: GlassStrings.of(context).capsLockOn),
      if (widget.layoutHint && _script != null)
        FieldNote(
          key: const ValueKey('keyboard-layout-note'),
          icon: Icons.keyboard_rounded,
          text: typedScriptHint(l10n, _script!),
        ),
    ];
    return TextFormField(
      controller: widget.controller,
      focusNode: _focus,
      obscureText: !_revealed,
      autocorrect: false,
      enableSuggestions: false,
      enableIMEPersonalizedLearning: false,
      keyboardType: TextInputType.visiblePassword,
      autofocus: widget.autofocus,
      enabled: widget.enabled,
      validator: widget.validator,
      onChanged: widget.onChanged,
      onFieldSubmitted: widget.onSubmitted,
      textInputAction: widget.textInputAction,
      autofillHints: widget.autofillHints,
      inputFormatters: [FilteringTextInputFormatter.singleLineFormatter],
      style: tokens.typography.mono.copyWith(color: tokens.palette.label),
      decoration: InputDecoration(
        labelText: widget.label,
        hintText: widget.hint,
        helperText: notes.isEmpty ? widget.helper : null,
        helper: notes.isEmpty
            ? null
            : Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: notes),
        suffixIcon: widget.allowReveal
            ? IconButton(
                tooltip: _revealed ? context.l10n.commonHide : context.l10n.commonShow,
                icon: Icon(_revealed ? Icons.visibility_off_rounded : Icons.visibility_rounded, size: 18),
                onPressed: () => setState(() => _revealed = !_revealed),
              )
            : null,
      ),
    );
  }
}
