import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_button.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Field heights (§4.4).
enum GlassFieldSize {
  md(28),
  lg(36);

  const GlassFieldSize(this.height);

  final double height;
}

/// Text field styling of LIQUID_GLASS_SPEC §4.4: always a `fill.field`, never
/// glass (also inside glass — vibrancy-on-glass rule). Radius `r.md`,
/// capsule for search. No border at rest; 2 px accent ring on focus; 1.5 px
/// danger border plus an icon + text error row.
///
/// [secret]: mono, obscured with an eye toggle, Caps Lock indicator, no
/// autocorrect/suggestions/IME learning. Secret fields belong on content
/// surfaces or the secure material only (asserted in debug).
class GlassField extends StatefulWidget {
  const GlassField({
    super.key,
    this.controller,
    this.focusNode,
    this.placeholder,
    this.size = GlassFieldSize.md,
    this.search = false,
    this.mono = false,
    this.secret = false,
    this.errorText,
    this.leadingIcon,
    this.trailing,
    this.onChanged,
    this.onSubmitted,
    this.autofocus = false,
    this.enabled = true,
    this.keyboardType,
    this.textInputAction,
    this.maxLines = 1,
    this.inputFormatters,
    this.autofillHints,
    this.semanticLabel,
    this.fieldKey,
  });

  final TextEditingController? controller;
  final FocusNode? focusNode;
  final String? placeholder;
  final GlassFieldSize size;

  /// Capsule search field.
  final bool search;
  final bool mono;
  final bool secret;
  final String? errorText;
  final IconData? leadingIcon;
  final Widget? trailing;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;
  final bool autofocus;
  final bool enabled;
  final TextInputType? keyboardType;
  final TextInputAction? textInputAction;
  final int maxLines;
  final List<TextInputFormatter>? inputFormatters;
  final Iterable<String>? autofillHints;
  final String? semanticLabel;

  /// Key of the inner `TextField` (for tests / `enterText`).
  final Key? fieldKey;

  @override
  State<GlassField> createState() => _GlassFieldState();
}

class _GlassFieldState extends State<GlassField> {
  FocusNode? _ownFocus;
  bool _focused = false;
  bool _revealed = false;
  bool _capsLock = false;

  FocusNode get _focus => widget.focusNode ?? (_ownFocus ??= FocusNode(debugLabel: 'GlassField'));

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocus);
    if (widget.secret) HardwareKeyboard.instance.addHandler(_onKey);
    _capsLock = HardwareKeyboard.instance.lockModesEnabled.contains(KeyboardLockMode.capsLock);
  }

  @override
  void didUpdateWidget(GlassField oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.focusNode != widget.focusNode) {
      (oldWidget.focusNode ?? _ownFocus)?.removeListener(_onFocus);
      _focus.addListener(_onFocus);
    }
    if (oldWidget.secret != widget.secret) {
      if (widget.secret) {
        HardwareKeyboard.instance.addHandler(_onKey);
      } else {
        HardwareKeyboard.instance.removeHandler(_onKey);
      }
    }
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
    _focus.removeListener(_onFocus);
    if (widget.secret) HardwareKeyboard.instance.removeHandler(_onKey);
    _ownFocus?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    assert(() {
      final onGlass = GlassOnGlass.maybeOf(context);
      if (widget.secret && onGlass != null && !onGlass.secure) {
        throw FlutterError(
          'Secret GlassFields must sit on a content surface or SecureSurface, '
          'not on ${onGlass.variant.name} glass (LIQUID_GLASS_SPEC §4.4).',
        );
      }
      return true;
    }());
    final tokens = GlassTokens.of(context);
    final strings = GlassStrings.of(context);
    final p = tokens.palette;
    final error = widget.errorText;
    final shape = widget.search ? const StadiumBorder() as OutlinedBorder : GlassRadii.shape(tokens.radii.md);
    final mono = widget.mono || widget.secret;
    final textStyle = (mono ? tokens.typography.mono : tokens.typography.body).copyWith(
      color: p.label.withValues(alpha: widget.enabled ? 1 : 0.38),
    );
    final height = widget.size.height;
    final iconSize = widget.size == GlassFieldSize.lg ? 18.0 : 16.0;

    final field = TextField(
      key: widget.fieldKey,
      controller: widget.controller,
      focusNode: _focus,
      autofocus: widget.autofocus,
      enabled: widget.enabled,
      style: textStyle,
      cursorColor: p.accent,
      obscureText: widget.secret && !_revealed,
      autocorrect: !widget.secret,
      enableSuggestions: !widget.secret,
      enableIMEPersonalizedLearning: !widget.secret,
      keyboardType: widget.secret ? TextInputType.visiblePassword : widget.keyboardType,
      textInputAction: widget.textInputAction,
      maxLines: widget.secret ? 1 : widget.maxLines,
      inputFormatters: widget.secret
          ? [FilteringTextInputFormatter.singleLineFormatter, ...?widget.inputFormatters]
          : widget.inputFormatters,
      autofillHints: widget.autofillHints,
      onChanged: widget.onChanged,
      onSubmitted: widget.onSubmitted,
      decoration: InputDecoration.collapsed(
        hintText: widget.placeholder,
        hintStyle: textStyle.copyWith(color: tokens.secondaryLabel),
      ),
    );

    final box = DecoratedBox(
      decoration: ShapeDecoration(
        color: tokens.surfaces.fillField,
        shape: shape.copyWith(side: error != null ? BorderSide(color: p.danger, width: 1.5) : BorderSide.none),
      ),
      child: ConstrainedBox(
        constraints: BoxConstraints(minHeight: height),
        child: Padding(
          padding: EdgeInsets.symmetric(
            horizontal: widget.search ? height / 2 : GlassSpacing.s8,
            vertical: widget.maxLines > 1 ? GlassSpacing.s6 : 0,
          ),
          child: Row(
            children: [
              if (widget.leadingIcon != null) ...[
                Icon(widget.leadingIcon, size: iconSize, color: tokens.secondaryLabel),
                const SizedBox(width: GlassSpacing.s6),
              ],
              Expanded(child: field),
              if (widget.secret)
                GlassIconButton(
                  icon: _revealed ? Icons.visibility_off_rounded : Icons.visibility_rounded,
                  tooltip: _revealed ? strings.hide : strings.show,
                  style: GlassIconButtonStyle.plain,
                  size: height - 6,
                  iconSize: iconSize,
                  onPressed: widget.enabled ? () => setState(() => _revealed = !_revealed) : null,
                ),
              ?widget.trailing,
            ],
          ),
        ),
      ),
    );

    final children = <Widget>[
      Material(
        type: MaterialType.transparency,
        child: GlassFocusRing(visible: _focused, shape: shape, child: box),
      ),
      if (widget.secret && _focused && _capsLock)
        _FieldNote(icon: Icons.warning_rounded, text: strings.capsLockOn, color: p.warning, style: tokens),
      if (error != null) _FieldNote(icon: Icons.error_rounded, text: error, color: p.danger, style: tokens),
    ];
    return Semantics(
      label: widget.semanticLabel,
      textField: true,
      child: children.length == 1
          ? children.first
          : Column(crossAxisAlignment: CrossAxisAlignment.stretch, mainAxisSize: MainAxisSize.min, children: children),
    );
  }
}

class _FieldNote extends StatelessWidget {
  const _FieldNote({required this.icon, required this.text, required this.color, required this.style});

  final IconData icon;
  final String text;
  final Color color;
  final GlassTokens style;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: GlassSpacing.s4),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(icon, size: 14, color: color),
        const SizedBox(width: GlassSpacing.s4),
        Expanded(
          child: Text(text, style: style.typography.callout.copyWith(color: color)),
        ),
      ],
    ),
  );
}
