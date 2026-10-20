import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_kubectl_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:url_launcher/url_launcher.dart';

/// Optional desktop helper. Discovery reads paths; a version check or package
/// command runs only after the corresponding explicit user action.
class KubeKubectlPanel extends ConsumerStatefulWidget {
  const KubeKubectlPanel({required this.onTerminalOpened, super.key});
  final ValueChanged<String> onTerminalOpened;
  @override
  ConsumerState<KubeKubectlPanel> createState() => _KubeKubectlPanelState();
}

class _KubeKubectlPanelState extends ConsumerState<KubeKubectlPanel> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubeKubectlService? _service;
  KubeFluxCliService? _plans;
  KubectlDiscovery? _discovery;
  FluxCliVersion? _version;
  String? _path, _method, _ticket;
  bool _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    final service = ref.read(kubeServiceProvider);
    if (service is KubeKubectlService && service is KubeFluxCliService) {
      _service = service as KubeKubectlService;
      _plans = service as KubeFluxCliService;
      unawaited(_discover());
    }
  }

  @override
  void dispose() {
    _cancel();
    super.dispose();
  }

  void _cancel() {
    final ticket = _ticket;
    _ticket = null;
    if (ticket != null) unawaited(_plans?.cancelFlux(ticket).catchError((Object _) {}));
  }

  Future<void> _work(Future<void> Function() action) async {
    if (!_current || _busy || _service == null) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      await action();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _discover() => _work(() async {
    final result = await _service!.discoverKubectl();
    if (!_current) return;
    setState(() {
      _discovery = result;
      _path = result.candidates.firstOrNull;
      _method = result.methods.firstOrNull;
      _version = null;
    });
  });

  Future<void> _probe() => _work(() async {
    final path = _path;
    if (path == null) return;
    final result = await _service!.probeKubectl(path);
    if (_current && _path == path) setState(() => _version = result);
  });

  Future<void> _install() => _work(() async {
    final method = _method;
    if (method == null) return;
    final review = await _service!.reviewKubectl(method);
    if (!mounted || !_current) {
      await _plans!.cancelFlux(review.ticket);
      return;
    }
    _ticket = review.ticket;
    try {
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => _KubectlReview(scope: _scope, review: review),
      );
      if (approved != true || !_current) return;
      final terminal = await _plans!.approveFlux(review);
      if (_current && terminal != null) widget.onTerminalOpened(terminal);
    } finally {
      _cancel();
    }
  });

  @override
  Widget build(BuildContext context) {
    ref.watch(kubeScopeProvider);
    ref.listen(kubeScopeProvider, (_, next) {
      if (!identical(_scope, next)) _cancel();
    });
    if (_service == null || !_current || _discovery?.supported == false) return const SizedBox.shrink();
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: 16),
        if (_busy) const LinearProgressIndicator(),
        if (_failed) Text(l.kubeFluxCliFailed),
        if (_discovery != null) ...[
          if (_path == null) Text(l.kubeKubectlMissing),
          if (_path != null) ...[
            DropdownButtonFormField<String>(
              key: ValueKey(_discovery),
              initialValue: _path,
              isExpanded: true,
              items: _discovery!.candidates
                  .map(
                    (path) => DropdownMenuItem(
                      value: path,
                      child: Text(path, maxLines: 1, overflow: TextOverflow.ellipsis),
                    ),
                  )
                  .toList(),
              onChanged: _busy
                  ? null
                  : (value) => setState(() {
                      _path = value;
                      _version = null;
                    }),
            ),
            if (_version != null) SelectableText('kubectl ${_version!.version}'), // l10n-ignore: CLI version.
            TextButton(onPressed: _busy ? null : _probe, child: Text(l.kubeKubectlCheck)),
          ],
          if (_path == null && _discovery!.methods.isNotEmpty) ...[
            DropdownButtonFormField<String>(
              initialValue: _method,
              isExpanded: true,
              items: _discovery!.methods
                  .map(
                    (method) => DropdownMenuItem(
                      value: method,
                      child: Text(switch (method) {
                        'homebrew' => 'Homebrew',
                        'winget' => 'winget',
                        'scoop' => 'Scoop',
                        'chocolatey' => 'Chocolatey',
                        _ => method,
                      }), // l10n-ignore: package manager names.
                    ),
                  )
                  .toList(),
              onChanged: _busy ? null : (value) => setState(() => _method = value),
            ),
            TextButton(onPressed: _busy ? null : _install, child: Text(l.kubeKubectlInstall)),
          ],
          Wrap(
            children: [
              TextButton(
                onPressed: _busy
                    ? null
                    : () => _work(() async {
                        final uri = Uri.parse(_discovery!.documentation);
                        if (uri.scheme != 'https' ||
                            uri.host != 'kubernetes.io' ||
                            !await launchUrl(uri, mode: LaunchMode.externalApplication)) {
                          throw StateError('kubectl documentation unavailable');
                        }
                      }),
                child: Text(l.kubeKubectlDocs),
              ),
              TextButton(onPressed: _busy ? null : _discover, child: Text(l.kubeRefreshResources)),
            ],
          ),
        ],
      ],
    );
  }
}

class _KubectlReview extends ConsumerWidget {
  const _KubectlReview({required this.scope, required this.review});
  final KubeScope scope;
  final FluxInstallReview review;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = identical(scope, ref.watch(kubeScopeProvider)) && scope.unlocked;
    if (!current) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!context.mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    }
    final l = context.l10n;
    return GlassDialog(
      title: l.kubeKubectlInstall,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.kubeKubectlPackageHelp),
            const SizedBox(height: 12),
            SelectableText(jsonEncode([review.executable, ...review.arguments])),
            SelectableText(review.source),
            if (review.sha256 != null) SelectableText('SHA-256: ${review.sha256}'), // l10n-ignore: algorithm.
          ],
        ),
      ),
      primaryAction: GlassButton.prominent(
        label: l.commonContinue,
        onPressed: current ? () => closeDialog(context, true) : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}
