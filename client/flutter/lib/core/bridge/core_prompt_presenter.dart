import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/prompt_service.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The [PromptService] of the running backend (`null` = prompts outside
/// terminal tabs are declined).
final promptServiceProvider = Provider<PromptService?>((ref) => ref.watch(appServicesProvider).prompts);

/// Minimal default presenter for core prompts raised outside terminal tabs
/// (SFTP, tunnels, exec): wrap the app content with it (e.g. in
/// `MaterialApp.builder`, below `Localizations`). While mounted, prompts are
/// shown one at a time as a modal card; without it they are declined.
///
/// Plain Material on purpose — the Liquid Glass dialog replaces it (the
/// texts reuse the terminal prompt strings). Security: the host key
/// fingerprint is always shown in full; secrets go into a [SecretText] and
/// the field is cleared after answering.
class CorePromptPresenter extends ConsumerStatefulWidget {
  const CorePromptPresenter({required this.child, super.key});

  final Widget child;

  @override
  ConsumerState<CorePromptPresenter> createState() => _CorePromptPresenterState();
}

class _CorePromptPresenterState extends ConsumerState<CorePromptPresenter> {
  PromptService? _service;
  void Function()? _detach;
  StreamSubscription<List<CorePrompt>>? _sub;
  List<CorePrompt> _pending = const [];
  final _secret = TextEditingController();
  bool _busy = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final service = ref.read(promptServiceProvider);
    if (identical(service, _service)) return;
    _unbind();
    _service = service;
    if (service != null) {
      _detach = service.attachPresenter();
      _sub = service.watchPending().listen((p) => setState(() => _pending = p));
    }
  }

  void _unbind() {
    unawaited(_sub?.cancel());
    _sub = null;
    _detach?.call();
    _detach = null;
  }

  @override
  void dispose() {
    _unbind();
    _secret.clear();
    _secret.dispose();
    super.dispose();
  }

  Future<void> _run(Future<void> Function(PromptService s) f) async {
    final s = _service;
    if (s == null || _busy) return;
    setState(() => _busy = true);
    try {
      await f(s);
    } on Object {
      // Timed out / already answered: the prompt disappears anyway.
    } finally {
      _secret.clear();
      if (mounted) setState(() => _busy = false);
    }
  }

  void _answerHostKey(HostKeyCorePrompt p, HostKeyDecision d) =>
      unawaited(_run((s) => s.answerHostKey(p.requestId, d)));

  void _answerSecret(CorePrompt p, {required bool cancel}) {
    final value = _secret.text;
    unawaited(_run((s) => s.answerSecret(p.requestId, cancel || value.isEmpty ? null : SecretText(value))));
  }

  @override
  Widget build(BuildContext context) {
    final prompt = _pending.firstOrNull;
    return Stack(
      children: [
        widget.child,
        if (prompt != null) ...[
          const ModalBarrier(dismissible: false, color: Color(0x66000000)),
          Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 480),
              child: Material(
                elevation: 12,
                borderRadius: BorderRadius.circular(12),
                child: Padding(padding: const EdgeInsets.all(20), child: _content(context, prompt)),
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _content(BuildContext context, CorePrompt prompt) {
    final l = AppLocalizations.of(context);
    final theme = Theme.of(context);
    switch (prompt) {
      case HostKeyCorePrompt():
        final host = prompt.hostName == null ? prompt.hostPattern : '${prompt.hostName} (${prompt.hostPattern})';
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.terminalUnknownHostTitle(host), style: theme.textTheme.titleMedium),
            const SizedBox(height: 12),
            Text(l.terminalUnknownHostMessage),
            const SizedBox(height: 12),
            SelectableText(
              '${prompt.keyType} ${prompt.fingerprintSha256}',
              style: const TextStyle(fontFamily: 'monospace'),
            ),
            if (prompt.otherKnownKeyTypes.isNotEmpty) ...[
              const SizedBox(height: 8),
              Text(prompt.otherKnownKeyTypes.join(', '), style: theme.textTheme.bodySmall),
            ],
            const SizedBox(height: 20),
            Wrap(
              alignment: WrapAlignment.end,
              spacing: 8,
              runSpacing: 8,
              children: [
                TextButton(
                  onPressed: _busy ? null : () => _answerHostKey(prompt, HostKeyDecision.reject),
                  child: Text(l.terminalHostKeyReject),
                ),
                OutlinedButton(
                  onPressed: _busy ? null : () => _answerHostKey(prompt, HostKeyDecision.acceptOnce),
                  child: Text(l.terminalHostKeyAcceptOnce),
                ),
                FilledButton(
                  onPressed: _busy ? null : () => _answerHostKey(prompt, HostKeyDecision.acceptAndSave),
                  child: Text(l.terminalHostKeyAcceptAndSave),
                ),
              ],
            ),
          ],
        );
      case PasswordCorePrompt() || PassphraseCorePrompt():
        final (title, label, retry) = switch (prompt) {
          PassphraseCorePrompt(:final credentialName, :final isRetry) => (
            credentialName,
            l.credentialDialogKeyPassphrase,
            isRetry,
          ),
          PasswordCorePrompt(:final hostName) => (hostName, l.terminalPasswordLabel, false),
          _ => ('', '', false),
        };
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: theme.textTheme.titleMedium),
            if (retry) ...[
              const SizedBox(height: 8),
              Text(l.terminalPasswordRetry, style: TextStyle(color: theme.colorScheme.error)),
            ],
            const SizedBox(height: 12),
            TextField(
              controller: _secret,
              autofocus: true,
              obscureText: true,
              enableSuggestions: false,
              autocorrect: false,
              decoration: InputDecoration(labelText: label, helperText: l.terminalPasswordHelper, helperMaxLines: 3),
              onSubmitted: (_) => _answerSecret(prompt, cancel: false),
            ),
            const SizedBox(height: 20),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TextButton(
                  onPressed: _busy ? null : () => _answerSecret(prompt, cancel: true),
                  child: Text(l.commonCancel),
                ),
                const SizedBox(width: 8),
                FilledButton(
                  onPressed: _busy ? null : () => _answerSecret(prompt, cancel: false),
                  child: Text(l.commonConnect),
                ),
              ],
            ),
          ],
        );
    }
  }
}
