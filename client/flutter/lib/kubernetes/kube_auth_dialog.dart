import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_auth_browser.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<bool> authenticateKubeConnection(BuildContext context, KubeConnection connection) async =>
    await showAppDialog<bool>(context, secure: true, builder: (_) => KubeAuthDialog(connection: connection)) ?? false;

class KubeAuthDialog extends ConsumerStatefulWidget {
  const KubeAuthDialog({super.key, required this.connection});
  final KubeConnection connection;
  @override
  ConsumerState<KubeAuthDialog> createState() => _KubeAuthDialogState();
}

class _KubeAuthDialogState extends ConsumerState<KubeAuthDialog> {
  late final KubeScope _scope;
  late final KubeAuthService? _service;
  late final KubeAuthBrowser _browser;
  KubeExecReview? _review;
  KubeOidcChallenge? _challenge;
  Timer? _expiry;
  bool _loading = true, _busy = false, _failed = false, _expired = false;
  bool get _exec => widget.connection.authentication == 'exec';
  bool get _mobileExec => _exec && AppPlatform.isMobile;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _service = ref.read(kubeAuthServiceProvider);
    _browser = ref.read(kubeAuthBrowserProvider);
    unawaited(_load());
  }

  void _cancel(String? ticket) {
    if (ticket == null) return;
    unawaited(_service?.cancelAuth(ticket).catchError((Object _) {}));
    unawaited(_browser.cancel(ticket).catchError((Object _) {}));
  }

  void _setExpiry(DateTime expiresAt) {
    _expiry?.cancel();
    final duration = expiresAt.difference(DateTime.now().toUtc());
    _expired = duration.isNegative || duration == Duration.zero;
    if (!_expired) {
      _expiry = Timer(duration, () {
        _cancel(_review?.ticket ?? _challenge?.ticket);
        if (mounted) setState(() => _expired = true);
      });
    }
  }

  Future<void> _load() async {
    if (_mobileExec || !_current) {
      if (mounted) setState(() => _loading = false);
      return;
    }
    try {
      final service = _service;
      if (service == null) throw StateError('Kubernetes authentication unavailable'); // l10n-ignore: diagnostic
      if (_exec) {
        final review = await service.reviewExec(widget.connection.id);
        if (!_current) {
          _cancel(review.ticket);
          review.dispose();
          return;
        }
        _review = review;
        _setExpiry(review.expiresAt);
      } else {
        final challenge = await service.beginOidc(widget.connection.id);
        if (!_current) {
          _cancel(challenge.ticket);
          return;
        }
        _challenge = challenge;
        _setExpiry(challenge.expiresAt);
      }
    } catch (_) {
      if (_current) _failed = true;
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _approve() async {
    final review = _review;
    if (!_current || _busy || _expired || review == null) return;
    setState(() => _busy = true);
    try {
      await _service!.approveExec(review.ticket);
      if (mounted && _current) closeDialog(context, true);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _signIn() async {
    final challenge = _challenge;
    if (!_current || _busy || _expired || challenge == null) return;
    setState(() => _busy = true);
    try {
      final callback = await _browser.open(challenge);
      if (!mounted || !_current) return;
      if (!challenge.loopback && callback == null) {
        closeDialog(context, false);
        return;
      }
      await _service!.completeOidc(challenge.ticket, callbackUrl: callback);
      if (mounted && _current) closeDialog(context, true);
    } catch (_) {
      _cancel(challenge.ticket);
      if (_current) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _expiry?.cancel();
    _cancel(_review?.ticket ?? _challenge?.ticket);
    _review?.dispose();
    _review = null;
    _challenge = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _cancel(_review?.ticket ?? _challenge?.ticket);
      _review?.dispose();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final review = _review;
    final canSubmit = !_loading && !_busy && !_expired && !_failed && _current && !_mobileExec;
    return GlassDialog(
      key: const ValueKey('kube-auth-dialog'),
      title: _exec ? l.kubeExecReviewTitle : l.kubeOidcTitle,
      width: 720,
      content: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: math.min(520, MediaQuery.sizeOf(context).height * .58)),
        child: SingleChildScrollView(
          key: const ValueKey('kube-auth-scroll'),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.connection.name, style: Theme.of(context).textTheme.titleSmall),
              const SizedBox(height: 12),
              Text(_mobileExec ? l.kubeExecMobile : (_exec ? l.kubeExecReviewHelp : l.kubeOidcHelp)),
              if (_loading || _busy) ...[
                const SizedBox(height: 12),
                const LinearProgressIndicator(),
                if (!_exec && _busy) Text(l.kubeOidcWaiting),
              ],
              if (_failed) ...[
                const SizedBox(height: 12),
                Text(l.kubeAuthFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ],
              if (_expired) ...[
                const SizedBox(height: 12),
                Text(l.kubeAuthExpired, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              ],
              if (review != null) ...[
                const SizedBox(height: 16),
                SelectableText(review.binaryPath, key: const ValueKey('kube-exec-path')),
                const SizedBox(height: 8),
                SelectableText(
                  /* l10n-ignore: SHA-256 algorithm identifier and reviewed executable digest. */ 'SHA-256: ${review.binarySha256}',
                ), // l10n-ignore: SHA-256 algorithm identifier and reviewed executable digest.
                const SizedBox(height: 16),
                Text(l.kubeExecArguments, style: Theme.of(context).textTheme.titleSmall),
                for (var i = 0; i < review.arguments.length; i++)
                  SelectableText('${i + 1}: ${jsonEncode(review.arguments[i])}'),
                const SizedBox(height: 16),
                Text(l.kubeExecEnvironment, style: Theme.of(context).textTheme.titleSmall),
                for (final entry in review.environment.entries)
                  SelectableText('${entry.key}=${jsonEncode(entry.value)}'),
              ],
            ],
          ),
        ),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('kube-auth-cancel'),
          label: l.commonCancel,
          autofocus: true,
          onPressed: () => closeDialog(context, false),
        ),
      ],
      primaryAction: _mobileExec
          ? null
          : GlassButton.prominent(
              key: const ValueKey('kube-auth-submit'),
              label: _exec ? l.kubeExecAllow : l.kubeOidcOpen,
              onPressed: canSubmit ? (_exec ? _approve : _signIn) : null,
            ),
    );
  }
}
