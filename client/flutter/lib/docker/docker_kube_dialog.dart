import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_kube_deployment.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DockerKubeDialog extends ConsumerStatefulWidget {
  const DockerKubeDialog({required this.source, required this.container, super.key});
  final DockerKubeSource source;
  final DockerContainer container;
  @override
  ConsumerState<DockerKubeDialog> createState() => _DockerKubeDialogState();
}

class _DockerKubeDialogState extends ConsumerState<DockerKubeDialog> with WidgetsBindingObserver {
  late final KubeScope _scope;
  late final DockerScope _dockerScope;
  late final KubeService _service = ref.read(kubeServiceProvider);
  final _name = TextEditingController(),
      _namespace = TextEditingController(),
      _image = TextEditingController(),
      _port = TextEditingController(),
      _pullSecret = TextEditingController(),
      _yaml = TextEditingController();
  final _parser = DockerKubeParser();
  List<KubeConnection> _contexts = const [];
  List<String> _namespaces = const [];
  List<KubeResourceKind> _kinds = const [];
  KubeConnection? _context;
  KubeSession? _session;
  DockerKubeDeployment? _deployment;
  ObjectId? _providerId;
  (AiProviderConfig, DockerKubeTarget)? _review;
  bool _busy = false, _newNamespace = false, _acknowledged = false, _namespaceCreated = false;
  bool? _ready;
  String? _error;
  int _generation = 0;
  // Cancelled on close, profile/lock change or app background.
  // ignore: cancel_subscriptions
  StreamSubscription<AiStreamEvent>? _subscription;
  Completer<String?>? _answer;
  bool get _current =>
      mounted &&
      _scope.unlocked &&
      _dockerScope.unlocked &&
      identical(_scope, ref.read(kubeScopeProvider)) &&
      identical(_dockerScope, ref.read(dockerScopeProvider));
  bool get _canWrite =>
      _current && _session != null && (ref.read(kubeWriteAccessProvider(_session!)).value?.canWrite ?? false);
  bool get _started => _deployment?.created.isNotEmpty ?? false;
  DockerKubeTarget get _target => DockerKubeTarget(
    name: _name.text.trim(),
    namespace: _namespace.text.trim(),
    image: _image.text.trim(),
    port: int.tryParse(_port.text.trim()) ?? 0,
    pullSecret: _pullSecret.text.trim(),
  );

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _dockerScope = ref.read(dockerScopeProvider);
    WidgetsBinding.instance.addObserver(this);
    var name = widget.container.name.toLowerCase().replaceAll(RegExp('[^a-z0-9-]'), '-');
    if (name.length > 50) name = name.substring(0, 50);
    _name.text = name.replaceAll(RegExp(r'-+$'), '');
    _port.text = (widget.source.ports.firstOrNull ?? 8080).toString();
    final image = widget.container.image;
    if (DockerKubeTarget.validImage(image)) _image.text = image;
    ref.listenManual(kubeScopeProvider, (_, next) {
      if (!identical(_scope, next)) _expire();
    });
    ref.listenManual(dockerScopeProvider, (_, next) {
      if (!identical(_dockerScope, next)) _expire();
    });
    if (!widget.source.unsupported) unawaited(_load());
  }

  Future<void> _load() async {
    if (_service is! KubeBrowserService || _service is! KubeManifestService) {
      if (_current) setState(() => _error = context.l10n.dockerKubeUnavailable);
      return;
    }
    setState(() => _busy = true);
    try {
      final contexts = await _service.inventory();
      if (_current) {
        setState(() {
          _contexts = contexts;
          if (contexts.isEmpty) _error = context.l10n.dockerKubeUnavailable;
        });
      }
    } catch (_) {
      if (_current) setState(() => _error = context.l10n.dockerKubeFailed);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  void _stopAi() {
    final subscription = _subscription;
    _subscription = null;
    if (subscription != null) unawaited(subscription.cancel());
    final answer = _answer;
    _answer = null;
    if (answer != null && !answer.isCompleted) answer.complete(null);
  }

  void _expire() {
    _generation++;
    _stopAi();
    _parser.cancel();
    _deployment?.cancel();
    _yaml.clear();
    _review = null;
    final session = _session;
    _session = null;
    if (session != null) unawaited(_disconnect(session));
    if (mounted) {
      setState(() => _busy = false);
      final route = ModalRoute.of(context);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    }
  }

  Future<void> _disconnect(KubeSession session) async {
    try {
      await (_service as KubeBrowserService).disconnect(session.id);
    } catch (_) {
      /* Vault may already be locked. */
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused || state == AppLifecycleState.hidden || state == AppLifecycleState.detached) {
      _expire();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _generation++;
    _stopAi();
    _parser.cancel();
    _deployment?.cancel();
    final session = _session;
    if (session != null) unawaited(_disconnect(session));
    for (final controller in [_name, _namespace, _image, _port, _pullSecret, _yaml]) {
      controller.clear();
      controller.dispose();
    }
    super.dispose();
  }

  void _changed({bool target = false}) {
    _deployment?.invalidate();
    _ready = null;
    _error = null;
    if (target) {
      _yaml.clear();
      _acknowledged = false;
    }
    setState(() {});
  }

  Future<void> _connect(KubeConnection connection) async {
    if (!_current || _busy || _started) return;
    final generation = ++_generation;
    final old = _session;
    _deployment?.cancel();
    setState(() {
      _session = null;
      _deployment = null;
      _context = connection;
      _namespaces = const [];
      _kinds = const [];
      _namespace.clear();
      _yaml.clear();
      _busy = true;
      _error = null;
      _newNamespace = false;
      _namespaceCreated = false;
      _acknowledged = false;
      _ready = null;
    });
    try {
      if (old != null) await _disconnect(old);
      if (!_current || generation != _generation) return;
      final browser = _service as KubeBrowserService;
      final session = await browser.connect(connection.id);
      if (!_current || generation != _generation) {
        await _disconnect(session);
        return;
      }
      _session = session; // Owned even if the following read fails.
      final kinds = await browser.discover(session.id);
      if (!_current || generation != _generation) return;
      final namespaces = await browser.namespaces(session.id);
      if (!_current || generation != _generation) return;
      setState(() {
        _namespaces = namespaces;
        _kinds = kinds;
        _namespace.text = namespaces.contains(connection.namespace)
            ? connection.namespace
            : namespaces.firstOrNull ?? '';
        _deployment = DockerKubeDeployment(
          service: _service as KubeManifestService,
          sessionId: session.id,
          kinds: kinds,
          current: () => _current && generation == _generation && _canWrite,
        );
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeFailed);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _createNamespace() async {
    if (!_current || _busy || !_canWrite || !DockerKubeTarget.validName(_namespace.text.trim())) return;
    final generation = _generation;
    final name = _namespace.text.trim();
    final text = jsonEncode({
      'apiVersion': 'v1',
      'kind': 'Namespace',
      'metadata': {'name': name},
    });
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final kind = _kinds.singleWhere((k) => k.id == DockerKubeParser.namespaceKind.id && k.verbs.contains('create'));
      final resource = KubeResourceRef(api: kind, name: name);
      final service = _service as KubeManifestService;
      Future<void> create({required bool dryRun}) async {
        // The native bridge consumes/wipes SecretText after each call.
        final manifest = SecretText(text);
        try {
          await service.createResource(_session!.id, resource, manifest, dryRun: dryRun);
        } finally {
          manifest.wipe();
        }
      }

      await create(dryRun: true);
      if (!_current || generation != _generation || !_canWrite) return;
      await create(dryRun: false);
      if (!_current || generation != _generation) return;
      _namespaces = [..._namespaces, name];
      _newNamespace = false;
      _namespaceCreated = true;
      _changed(target: true);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeFailed);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  void _template() {
    if (!_target.valid) {
      setState(() => _error = context.l10n.dockerKubeInvalidTarget);
      return;
    }
    _yaml.text = _target.template;
    _changed();
  }

  Future<void> _generate(AiProviderConfig provider, DockerKubeTarget target) async {
    if (!_current || _busy || !_target.valid) return;
    final generation = _generation;
    setState(() {
      _review = null;
      _busy = true;
      _error = null;
      _deployment?.invalidate();
    });
    final answer = _answer = Completer<String?>();
    try {
      _subscription = ref
          .read(aiServiceProvider)
          .chat(providerId: provider.id, history: [ChatMessage.now(ChatRole.user, target.prompt)])
          .listen(
            (event) {
              if (!_current || generation != _generation || answer.isCompleted) return;
              if (event is AiCompleted) answer.complete(event.fullText.length <= 65536 ? event.fullText : null);
              if (event is AiFailed) answer.complete(null);
            },
            onError: (Object _) {
              if (!answer.isCompleted) answer.complete(null);
            },
            onDone: () {
              if (!answer.isCompleted) answer.complete(null);
            },
          );
      final text = await answer.future.timeout(const Duration(seconds: 120), onTimeout: () => null);
      if (!_current || generation != _generation) return;
      if (text == null) {
        setState(() => _error = context.l10n.dockerKubeAiFailed);
        return;
      }
      final documents = await _parser.parse(text, target);
      if (!_current || generation != _generation) return;
      if (documents == null) {
        setState(() => _error = context.l10n.dockerKubeManifestRejected);
        return;
      }
      // Keep the reviewed source, not a silent rewritten manifest.
      setState(() {
        _yaml.text = text;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeAiFailed);
    } finally {
      _stopAi();
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _validate() async {
    if (!_current || _busy || !_canWrite || !_acknowledged || _newNamespace || _deployment == null || _started) return;
    final generation = _generation;
    setState(() {
      _busy = true;
      _error = null;
      _deployment!.invalidate();
    });
    try {
      final documents = await _parser.parse(_yaml.text, _target);
      if (!_current || generation != _generation) return;
      if (documents == null) {
        setState(() => _error = context.l10n.dockerKubeManifestRejected);
        return;
      }
      await _deployment!.validate(documents);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeFailed);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _deploy() async {
    if (!_current || _busy || !_canWrite || !_acknowledged || !(_deployment?.validated ?? false)) return;
    final generation = _generation;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await _deployment!.deploy(
        onProgress: () {
          if (_current) setState(() {});
        },
      );
      if (_current && generation == _generation) await _status();
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeFailed);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _status() async {
    if (!_current || !_started) return;
    final generation = _generation;
    try {
      final detail = await (_service as KubeBrowserService).getResource(_session!.id, _deployment!.created.first);
      if (!_current || generation != _generation) return;
      final status = detail.object['status'], metadata = detail.object['metadata'];
      setState(
        () => _ready =
            status is Map &&
            metadata is Map &&
            status['observedGeneration'] is num &&
            metadata['generation'] is num &&
            (status['observedGeneration'] as num) >= (metadata['generation'] as num) &&
            status['updatedReplicas'] == 1 &&
            status['readyReplicas'] == 1 &&
            status['availableReplicas'] == 1,
      );
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeFailed);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (widget.source.blocker case final blocker?) {
      return GlassDialog(
        title: l.dockerKubeDeploy,
        icon: Icons.info_outline,
        width: 560,
        content: !_current
            ? null
            : SingleChildScrollView(
                child: Text(switch (blocker) {
                  DockerKubeBlocker.mounts => l.dockerKubeMountsBlocked,
                  DockerKubeBlocker.unsupportedConfiguration => l.dockerKubeUnsupported,
                }),
              ),
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      );
    }

    final providers = ref.watch(aiProvidersProvider).value ?? const <AiProviderConfig>[];
    final selected =
        providers.where((p) => p.id == _providerId).firstOrNull ??
        providers.where((p) => p.isDefault).firstOrNull ??
        providers.firstOrNull;
    final session = _session;
    if (session != null) ref.watch(kubeWriteAccessProvider(session));
    final enabled = !_busy && _review == null && !_started;
    final review = _review;
    Widget field(TextEditingController controller, String key, String label) => TextField(
      key: ValueKey('docker-kube-$key'),
      controller: controller,
      enabled: enabled,
      autocorrect: false,
      enableSuggestions: false,
      enableIMEPersonalizedLearning: false,
      decoration: InputDecoration(labelText: label),
      onChanged: (_) => _changed(target: true),
    );
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        title: l.dockerKubeDeploy,
        width: 960,
        content: SizedBox(
          height: 590,
          child: !_current
              ? const SizedBox.shrink()
              : ListView(
                  children: [
                    Text(l.dockerKubeIntro),
                    DropdownButtonFormField<String>(
                      key: const ValueKey('docker-kube-context'),
                      initialValue: _context?.id,
                      decoration: InputDecoration(labelText: l.dockerKubeContext),
                      items: [for (final c in _contexts) DropdownMenuItem(value: c.id, child: Text(c.name))],
                      onChanged: enabled
                          ? (id) {
                              if (id != null) unawaited(_connect(_contexts.firstWhere((c) => c.id == id)));
                            }
                          : null,
                    ),
                    if (session != null) ...[
                      KubeWriteBanner(session: session, contextName: _context!.name),
                      if (!_newNamespace)
                        DropdownButtonFormField<String>(
                          key: ValueKey('docker-kube-namespace-${session.id}-${_namespace.text}'),
                          initialValue: _namespaces.contains(_namespace.text) ? _namespace.text : null,
                          decoration: InputDecoration(labelText: l.kubeNamespace),
                          items: [for (final ns in _namespaces) DropdownMenuItem(value: ns, child: Text(ns))],
                          onChanged: enabled
                              ? (value) {
                                  if (value != null) {
                                    _namespace.text = value;
                                    _changed(target: true);
                                  }
                                }
                              : null,
                        ),
                      CheckboxListTile(
                        key: const ValueKey('docker-kube-new-namespace'),
                        title: Text(l.dockerKubeNewNamespace),
                        value: _newNamespace,
                        onChanged: enabled
                            ? (value) {
                                _newNamespace = value ?? false;
                                _namespace.clear();
                                _changed(target: true);
                              }
                            : null,
                      ),
                      if (_newNamespace) ...[
                        field(_namespace, 'namespace-name', l.kubeNamespace),
                        Text(l.dockerKubeNamespaceHelp),
                        Align(
                          alignment: Alignment.centerLeft,
                          child: GlassButton(
                            key: const ValueKey('docker-kube-create-namespace'),
                            label: l.dockerKubeCreateNamespace,
                            onPressed: enabled && _canWrite && DockerKubeTarget.validName(_namespace.text.trim())
                                ? _createNamespace
                                : null,
                          ),
                        ),
                      ],
                      if (_namespaceCreated)
                        Text(l.dockerKubeNamespaceCreated, key: const ValueKey('docker-kube-namespace-created')),
                    ],
                    field(_name, 'name', l.kubeName),
                    field(_image, 'image', l.dockerKubeImage),
                    Text(l.dockerKubeImageHelp),
                    field(_port, 'port', l.dockerKubePort),
                    field(_pullSecret, 'pull-secret', l.dockerKubePullSecret),
                    DropdownButtonFormField<String>(
                      key: const ValueKey('docker-kube-provider'),
                      initialValue: selected?.id.value,
                      decoration: InputDecoration(labelText: l.aiProviderDialogProviderLabel),
                      items: [for (final p in providers) DropdownMenuItem(value: p.id.value, child: Text(p.name))],
                      onChanged: enabled
                          ? (value) => setState(() => _providerId = value == null ? null : ObjectId(value))
                          : null,
                    ),
                    Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      children: [
                        GlassButton(
                          key: const ValueKey('docker-kube-generate'),
                          label: l.dockerKubeGenerate,
                          icon: Icons.auto_awesome,
                          onPressed: enabled && _target.valid && selected != null
                              ? () => setState(() => _review = (selected, _target))
                              : null,
                        ),
                        GlassButton(
                          key: const ValueKey('docker-kube-template'),
                          label: l.dockerKubeTemplate,
                          onPressed: enabled ? _template : null,
                        ),
                      ],
                    ),
                    if (review != null) ...[
                      Text(review.$1.name),
                      Text(l.dockerKubeAiReview),
                      SizedBox(height: 220, child: SingleChildScrollView(child: SelectableText(review.$2.template))),
                      Wrap(
                        spacing: 8,
                        children: [
                          GlassButton(
                            key: const ValueKey('docker-kube-send-ai'),
                            label: l.dockerKubeSendAi,
                            onPressed: () => _generate(review.$1, review.$2),
                          ),
                          GlassButton(label: l.commonCancel, onPressed: () => setState(() => _review = null)),
                        ],
                      ),
                    ],
                    TextField(
                      key: const ValueKey('docker-kube-yaml'),
                      controller: _yaml,
                      enabled: enabled,
                      minLines: 8,
                      maxLines: 16,
                      maxLength: 65536,
                      autocorrect: false,
                      enableSuggestions: false,
                      enableIMEPersonalizedLearning: false,
                      style: GlassTokens.of(context).typography.mono,
                      decoration: InputDecoration(labelText: l.kubeYaml, counterText: ''),
                      onChanged: (_) => _changed(),
                    ),
                    CheckboxListTile(
                      key: const ValueKey('docker-kube-acknowledge'),
                      title: Text(l.dockerKubeAcknowledge),
                      value: _acknowledged,
                      onChanged: enabled
                          ? (value) {
                              _acknowledged = value ?? false;
                              _changed();
                            }
                          : null,
                    ),
                    if (_deployment?.validated ?? false)
                      Text(l.dockerKubeValidated, key: const ValueKey('docker-kube-validated')),
                    if (_busy) const LinearProgressIndicator(),
                    if (_error != null) Text(_error!, key: const ValueKey('docker-kube-error')),
                    if (_started) ...[
                      Text(l.dockerKubeCreated),
                      for (final resource in _deployment!.created)
                        SelectableText('${resource.api.kind} ${resource.namespace}/${resource.name}'),
                      Text(
                        _ready == true ? l.dockerKubeReady : l.dockerKubePending,
                        key: const ValueKey('docker-kube-status'),
                      ),
                      Align(
                        alignment: Alignment.centerLeft,
                        child: GlassButton(label: l.dockerKubeCheckStatus, onPressed: _busy ? null : _status),
                      ),
                    ],
                  ],
                ),
        ),
        primaryAction: GlassButton.prominent(
          key: const ValueKey('docker-kube-deploy'),
          label: l.dockerKubeDeploy,
          onPressed: enabled && _canWrite && _acknowledged && (_deployment?.validated ?? false) ? _deploy : null,
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('docker-kube-validate'),
            label: l.kubeDryRun,
            onPressed: enabled && _canWrite && _acknowledged && !_newNamespace && _target.valid && _yaml.text.isNotEmpty
                ? _validate
                : null,
          ),
          GlassButton(label: l.commonClose, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
      ),
    );
  }
}
