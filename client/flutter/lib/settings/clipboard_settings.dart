import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class MultilinePasteSetting extends ConsumerWidget {
  const MultilinePasteSetting({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.read(settingsServiceProvider);
    return CheckboxListTile(
      key: const ValueKey('terminal-allow-multiline-paste'),
      contentPadding: EdgeInsets.zero,
      controlAffinity: ListTileControlAffinity.leading,
      title: Text(context.l10n.terminalAllowMultilinePaste),
      value: ref.watch(localSettingsProvider.select((s) => s.value?.allowMultilinePaste)) ?? false,
      onChanged: (value) => runWithFeedback(
        context,
        () => settings.updateLocal(settings.currentLocal.copyWith(allowMultilinePaste: value)),
      ),
    );
  }
}

/// Android only (S8, ТР-4): clear the clipboard at the deadline even when the
/// app is in the background and cannot check that it still holds our secret.
/// A native device preference: the alarm receiver reads it without Flutter.
/// On by default.
class ClipboardBackgroundClearSettings extends StatefulWidget {
  const ClipboardBackgroundClearSettings({super.key});

  @override
  State<ClipboardBackgroundClearSettings> createState() => _ClipboardBackgroundClearSettingsState();
}

class _ClipboardBackgroundClearSettingsState extends State<ClipboardBackgroundClearSettings> {
  bool? _enabled;
  bool _busy = false;
  bool _error = false;

  bool get _android => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  @override
  void initState() {
    super.initState();
    if (_android) _run(() => SecureClipboard.channel.invokeMethod<bool>('getBackgroundClear'));
  }

  Future<void> _run(Future<bool?> Function() action) async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = false;
    });
    try {
      final enabled = await action();
      if (mounted) {
        setState(() {
          _error = enabled == null;
          if (enabled != null) _enabled = enabled;
        });
      }
    } on PlatformException {
      if (mounted) setState(() => _error = true);
    } on MissingPluginException {
      if (mounted) setState(() => _error = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!_android) return const SizedBox.shrink();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      key: const ValueKey('clipboard-background-clear-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile.adaptive(
          key: const ValueKey('clipboard-background-clear'),
          contentPadding: EdgeInsets.zero,
          title: Text(l.settingsClipboardBackgroundClear),
          subtitle: Text(l.settingsClipboardBackgroundClearHelp),
          value: _enabled ?? true,
          onChanged: !_busy && _enabled != null
              ? (enabled) =>
                    _run(() => SecureClipboard.channel.invokeMethod<bool>('setBackgroundClear', {'enabled': enabled}))
              : null,
        ),
        if (_error)
          Text(
            l.settingsClipboardBackgroundClearError,
            key: const ValueKey('clipboard-background-clear-error'),
            style: TextStyle(color: tokens.palette.danger),
          ),
      ],
    );
  }
}
