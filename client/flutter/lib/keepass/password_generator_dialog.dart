import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/keepass/password_generator.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<SecretText?> showPasswordGenerator(
  BuildContext context, {
  required ValueListenable<bool> valid,
  bool allowUse = false,
}) => showAppDialog<SecretText>(
  context,
  secure: true,
  builder: (_) => PasswordGeneratorDialog(valid: valid, allowUse: allowUse),
);

class PasswordGeneratorDialog extends ConsumerStatefulWidget {
  const PasswordGeneratorDialog({super.key, required this.valid, this.allowUse = false});
  final ValueListenable<bool> valid;
  final bool allowUse;
  @override
  ConsumerState<PasswordGeneratorDialog> createState() => _PasswordGeneratorDialogState();
}

class _PasswordGeneratorDialogState extends ConsumerState<PasswordGeneratorDialog> {
  final _value = TextEditingController();
  final _length = TextEditingController(text: '20');
  bool _upper = true, _lower = true, _digits = true, _symbols = true, _similar = false, _revealed = false;
  String? _error;
  double _bits = 0;
  PasswordGeneratorOptions get _options => PasswordGeneratorOptions(
    length: int.tryParse(_length.text) ?? 0,
    uppercase: _upper,
    lowercase: _lower,
    digits: _digits,
    symbols: _symbols,
    excludeSimilar: _similar,
  );
  @override
  void initState() {
    super.initState();
    widget.valid.addListener(_scope);
    _generate();
  }

  void _scope() {
    if (!widget.valid.value) {
      _value.clear();
      _revealed = false;
      if (mounted) setState(() {});
    }
  }

  void _generate() {
    _value.clear();
    _bits = 0;
    _error = null;
    if (!widget.valid.value) return;
    final options = _options;
    if (options.length < 8 || options.length > 128) {
      _error = 'length';
      return;
    }
    if (!_upper && !_lower && !_digits && !_symbols) {
      _error = 'groups';
      return;
    }
    final generated = options.generate();
    try {
      _value.text = generated.expose();
      _bits = options.entropyBits;
    } finally {
      generated.wipe();
    }
  }

  void _change(VoidCallback action) => setState(() {
    action();
    _generate();
  });
  @override
  void dispose() {
    widget.valid.removeListener(_scope);
    _value.clear();
    _value.dispose();
    _length.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final ready = widget.valid.value && _error == null && _value.text.isNotEmpty;
    Widget choice(String key, String label, bool value, ValueChanged<bool> update) => CheckboxListTile(
      key: ValueKey(key),
      dense: true,
      contentPadding: EdgeInsets.zero,
      title: Text(label),
      value: value,
      onChanged: (v) => _change(() => update(v ?? false)),
    );
    return GlassDialog(
      key: const ValueKey('keepass-generator'),
      title: l.keepassGenerator,
      width: 600,
      content: !widget.valid.value
          ? Text(l.keepassClosed)
          : SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                spacing: 12,
                children: [
                  RevealedSecret(
                    visible: _revealed,
                    child: TextField(
                      key: const ValueKey('generator-value'),
                      controller: _value,
                      readOnly: true,
                      obscureText: !_revealed,
                      enableInteractiveSelection: false,
                      autocorrect: false,
                      enableSuggestions: false,
                      decoration: InputDecoration(
                        labelText: l.keepassEntryPassword,
                        suffixIcon: GlassIconButton(
                          key: const ValueKey('generator-reveal'),
                          icon: _revealed ? Icons.visibility_off_outlined : Icons.visibility_outlined,
                          tooltip: _revealed ? l.commonHide : l.commonShow,
                          onPressed: ready ? () => setState(() => _revealed = !_revealed) : null,
                        ),
                      ),
                    ),
                  ),
                  LinearProgressIndicator(value: (_bits / 128).clamp(0, 1)),
                  Text(l.keepassGeneratorEntropy(_bits.toStringAsFixed(1))),
                  TextField(
                    key: const ValueKey('generator-length'),
                    controller: _length,
                    decoration: InputDecoration(labelText: l.keepassGeneratorLength),
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(3)],
                    onChanged: (_) => _change(() {}),
                  ),
                  Slider(
                    key: const ValueKey('generator-length-slider'),
                    value: (_options.length).clamp(8, 128).toDouble(),
                    min: 8,
                    max: 128,
                    divisions: 120,
                    label: '${_options.length}',
                    onChanged: (v) => _change(() => _length.text = v.round().toString()),
                  ),
                  choice('generator-upper', l.keepassGeneratorUpper, _upper, (v) => _upper = v),
                  choice('generator-lower', l.keepassGeneratorLower, _lower, (v) => _lower = v),
                  choice('generator-digits', l.keepassGeneratorDigits, _digits, (v) => _digits = v),
                  choice('generator-symbols', l.keepassGeneratorSymbols, _symbols, (v) => _symbols = v),
                  choice('generator-similar', l.keepassGeneratorSimilar, _similar, (v) => _similar = v),
                  if (_error != null)
                    Text(
                      _error == 'length' ? l.keepassGeneratorLengthError : l.keepassGeneratorGroupsError,
                      style: TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      GlassButton(
                        key: const ValueKey('generator-regenerate'),
                        label: l.keepassGeneratorRegenerate,
                        icon: Icons.refresh_rounded,
                        onPressed: () => _change(() {}),
                      ),
                      GlassButton(
                        key: const ValueKey('generator-copy'),
                        label: l.keepassCopyPassword,
                        icon: Icons.copy_outlined,
                        onPressed: ready
                            ? () => copySecretWithNotice(
                                context,
                                ref,
                                _value.text,
                                isCurrent: () => mounted && widget.valid.value,
                              )
                            : null,
                      ),
                    ],
                  ),
                ],
              ),
            ),
      primaryAction: widget.allowUse
          ? GlassButton.prominent(
              key: const ValueKey('generator-use'),
              label: l.keepassGeneratorUse,
              onPressed: ready ? () => closeDialog(context, SecretText(_value.text)) : null,
            )
          : GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      secondaryActions: widget.allowUse
          ? [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))]
          : const [],
    );
  }
}
