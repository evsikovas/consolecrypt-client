import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/secret_selectable_text.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeSecretDialog extends ConsumerStatefulWidget {
  const KubeSecretDialog({required this.session, required this.resource, required this.service, super.key});
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeManifestService service;
  @override
  ConsumerState<KubeSecretDialog> createState() => _KubeSecretDialogState();
}

class _KubeSecretDialogState extends ConsumerState<KubeSecretDialog> with WidgetsBindingObserver {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _values = <String, SecretText>{}, _binary = <String>{};
  // The app clipboard with the configured auto-clear; closing the dialog,
  // locking or leaving the app clears a value copied here sooner.
  late final SecureClipboard _clipboard;
  bool _copied = false;
  Timer? _timer;
  bool _busy = false, _failed = false, _expired = false;
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    _clipboard = ref.read(secureClipboardProvider);
    WidgetsBinding.instance.addObserver(this);
  }

  void _wipe() {
    _generation++;
    _timer?.cancel();
    _timer = null;
    for (final value in _values.values) {
      value.wipe();
    }
    _values.clear();
    _binary.clear();
  }

  void _clearCopied() {
    if (!_copied) return;
    _copied = false;
    unawaited(_clipboard.clearNow());
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _wipe();
    _clearCopied();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused || state == AppLifecycleState.hidden || state == AppLifecycleState.detached) {
      _wipe();
      _clearCopied();
      if (mounted) closeDialog<void>(context);
    }
  }

  Future<void> _reveal() async {
    if (!_current || _busy) return;
    _wipe();
    final generation = _generation;
    setState(() {
      _busy = true;
      _failed = false;
      _expired = false;
    });
    try {
      final secret = await widget.service.revealSecret(widget.session.id, widget.resource);
      try {
        if (!_current || generation != _generation) return;
        final values = jsonDecode(secret.expose()) as Map<String, dynamic>;
        for (final entry in values.entries) {
          if (entry.value is! String) continue;
          final bytes = base64Decode(entry.value as String);
          try {
            utf8.decode(bytes);
            _values[entry.key] = SecretText.fromBytes(bytes);
          } on FormatException {
            _values[entry.key] = SecretText(entry.value as String);
            _binary.add(entry.key);
          } finally {
            bytes.fillRange(0, bytes.length, 0);
          }
        }
        values.clear();
      } finally {
        secret.wipe();
      }
      if (_current && generation == _generation) {
        setState(() {});
        _timer = Timer(const Duration(seconds: 30), () {
          _wipe();
          if (mounted) setState(() => _expired = true);
        });
      }
    } catch (_) {
      _wipe();
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _copy(String key) async {
    final value = _values[key];
    if (!_current || value == null || value.isWiped) return;
    await _copyText(value.expose(), key);
  }

  /// The whole value (copy button) or a selection of it (its Copy menu).
  Future<void> _copyText(String text, String key) async {
    if (!_current) return;
    final generation = _generation;
    _copied = true;
    await copySecretWithNotice(context, ref, text, what: key, isCurrent: () => _current && generation == _generation);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _wipe();
      _clearCopied();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final keys = _values.keys.toList()..sort();
    return GlassDialog(
      title: '${l.kubeRevealSecret} · ${widget.resource.name}',
      width: 850,
      content: SizedBox(
        height: 420,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxHeight: 160),
                    child: SingleChildScrollView(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(l.kubeRevealSecretHelp),
                          if (_expired) Text(l.kubeSecretExpired),
                          if (_failed) Text(l.kubeRequestFailed),
                        ],
                      ),
                    ),
                  ),
                  if (_busy) const LinearProgressIndicator(),
                  if (keys.isEmpty)
                    Align(
                      alignment: Alignment.centerLeft,
                      child: GlassButton(
                        key: const ValueKey('kube-secret-reveal'),
                        label: l.kubeRevealSecret,
                        icon: Icons.visibility_outlined,
                        onPressed: _busy ? null : _reveal,
                      ),
                    ),
                  Expanded(
                    child: RevealedSecret(
                      visible: keys.isNotEmpty,
                      child: ListView.builder(
                        itemCount: keys.length,
                        itemBuilder: (context, index) {
                          final key = keys[index];
                          return Padding(
                            padding: const EdgeInsets.symmetric(vertical: 8),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                Row(
                                  children: [
                                    Expanded(
                                      child: Text(
                                        /* l10n-ignore: Secret key identifier with standard Base64 encoding marker. */ '$key${_binary.contains(key) ? ' · Base64' : ''}',
                                      ),
                                    ), // l10n-ignore: Secret key identifier with standard Base64 encoding marker.
                                    GlassIconButton(
                                      key: ValueKey('kube-secret-copy-$key'),
                                      icon: Icons.copy,
                                      tooltip: l.commonCopy,
                                      onPressed: () => _copy(key),
                                    ),
                                  ],
                                ),
                                SecretSelectableText(
                                  _values[key]!.expose(),
                                  onCopy: (selected) => _copyText(selected, key),
                                ),
                              ],
                            ),
                          );
                        },
                      ),
                    ),
                  ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
    );
  }
}
