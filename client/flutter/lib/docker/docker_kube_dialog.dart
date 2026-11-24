import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_kube_deployment.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/registry_publisher.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DockerKubeDialog extends ConsumerStatefulWidget {
  const DockerKubeDialog({
    required this.source,
    required this.container,
    this.resolveImage,
    this.imageKey,
    this.publishImage,
    this.buildImage,
    this.remoteBuild = false,
    this.engineName = '',
    super.key,
  });
  final DockerKubeSource source;
  final DockerImageKey? imageKey;
  final String engineName;
  final bool remoteBuild;
  final Future<String> Function(Credential, String, String)? publishImage;
  final Future<String> Function(Credential, String, String, DockerBuildContext)? buildImage;
  final DockerContainer container;
  final Future<DockerKubeSource> Function()? resolveImage;
  @override
  ConsumerState<DockerKubeDialog> createState() => _DockerKubeDialogState();
}

class _DockerKubeDialogState extends ConsumerState<DockerKubeDialog> with WidgetsBindingObserver {
  late final KubeScope _scope;
  late final DockerScope _dockerScope;
  late final KubeService _service = ref.read(kubeServiceProvider);
  late DockerKubeSource _source = widget.source;
  bool _resolvingImage = false;
  bool _imageEdited = false;
  String? _pinnedImage;
  final _formScroll = ScrollController();
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
  ({AiProviderConfig provider, DockerKubeTarget target, String prompt})? _review;
  bool _publishing = false;
  bool _busy = false, _newNamespace = false, _acknowledged = false, _namespaceCreated = false;
  DockerKubeRollout? _rollout, _lastPodProblem;
  Timer? _statusTimer;
  bool _readingStatus = false;
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
    startup: _source.startup,
    source: _source,
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
    final published = ref.read(dockerPublishedImagesProvider)[widget.imageKey];
    _useKnownImage(published ?? _source.image);
    ref.listenManual(kubeScopeProvider, (_, next) {
      if (!identical(_scope, next)) _expire();
    });
    ref.listenManual(dockerScopeProvider, (_, next) {
      if (!identical(_dockerScope, next)) _expire();
    });
    if (_current && !_source.unsupported) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_current) unawaited(_load());
      });
      if (widget.resolveImage case final resolve?) unawaited(_resolveImage(resolve));
    }
  }

  void _useKnownImage(String? reference) {
    if (reference == null) return;
    var tagged = reference.split('@').first;
    if (reference.contains('@') && !tagged.split('/').last.contains(':')) {
      final original = DockerKubeTarget.dockerImage(_source.sourceImage ?? '');
      if (original == null || original.contains('@') || original.substring(0, original.lastIndexOf(':')) != tagged) {
        _pinnedImage = null;
        _image.text = reference;
        return;
      }
      tagged = original;
    }
    _pinnedImage = reference.contains('@') ? "$tagged@${reference.split('@').last}" : null;
    _image.text = tagged;
  }

  void _selectPin(bool pinned) {
    final previous = _target;
    final wasTemplate = previous.valid && _yaml.text == previous.template;
    final reference = _pinnedImage!;
    _imageEdited = true;
    _image.text = pinned ? reference : reference.split('@').first;
    _changed(target: true);
    if (wasTemplate && _target.valid) _yaml.text = _target.template;
  }

  Future<void> _resolveImage(Future<DockerKubeSource> Function() resolve) async {
    _resolvingImage = true;
    try {
      final source = await resolve().timeout(const Duration(seconds: 10));
      if (!_current) return;
      _source = source;
      if (!_imageEdited && _image.text.isEmpty) _useKnownImage(source.image);
    } catch (_) {
      // A published reference can still be supplied manually.
    } finally {
      if (_current) setState(() => _resolvingImage = false);
    }
  }

  Future<void> _publish(Credential credential, String path, String tag, DockerBuildContext? build) async {
    if (!_current || _busy || _started || widget.publishImage == null) return;
    setState(() {
      _busy = true;
      _publishing = true;
      _error = null;
    });
    try {
      final image = build == null
          ? await widget.publishImage!(credential, path, tag)
          : await widget.buildImage!(credential, path, tag, build);
      if (!_current) return;
      if (!DockerKubeTarget.validImage(image) || !image.startsWith('${credential.registry}/$path@sha256:')) {
        throw const FormatException();
      }
      final previous = _target;
      final usingTemplate = _yaml.text.isNotEmpty && previous.valid && _yaml.text == previous.template;
      _imageEdited = true;
      _useKnownImage('${credential.registry}/$path:$tag@${image.split('@').last}');
      if (widget.imageKey case final key?) ref.read(dockerPublishedImagesProvider)[key] = _pinnedImage!;
      _changed(target: true);
      if (usingTemplate && _target.valid) _yaml.text = _target.template;
      if (mounted) showSnack(context, context.l10n.registryPublished);
    } catch (error) {
      if (mounted && _current) {
        final l = context.l10n;
        setState(
          () => _error = build == null
              ? l.registryPublishFailed
              : error is AppException && error.args['rule'] == 'build_context'
              ? l.registryBuildContextError
              : error is AppException && error.args['rule'] == 'buildx_remote_tools'
              ? l.registryRemoteTools
              : l.registryBuildFailed,
        );
      }
    } finally {
      if (_current) {
        setState(() {
          _busy = false;
          _publishing = false;
        });
      }
    }
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
    _statusTimer?.cancel();
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
    _formScroll.dispose();
    _generation++;
    _statusTimer?.cancel();
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
    _rollout = null;
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
      _rollout = null;
      _lastPodProblem = null;
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

  Future<void> _generate() async {
    final review = _review;
    if (!_current || _busy || review == null || !_target.valid || review.prompt.trim().isEmpty) return;
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
          .chat(providerId: review.provider.id, history: [ChatMessage.now(ChatRole.user, review.prompt)])
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
      final yaml = await _parser.mergeAi(text, review.target);
      if (!_current || generation != _generation) return;
      if (yaml == null) {
        setState(() => _error = context.l10n.dockerKubeManifestRejected);
        return;
      }
      setState(() {
        _yaml.text = yaml;
        _acknowledged = false;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _error = context.l10n.dockerKubeAiFailed);
    } finally {
      _stopAi();
      if (_current && generation == _generation) {
        setState(() {
          _busy = false;
          if (_error != null) _review = review;
        });
      }
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
    } catch (error) {
      if (_current && generation == _generation) setState(() => _error = _createError(error));
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
      if (_current && generation == _generation) unawaited(_status());
    } catch (error) {
      if (_current && generation == _generation) setState(() => _error = _createError(error));
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  String _createError(Object error) {
    final l = context.l10n;
    if (error is AppException) {
      return switch (error.args['rule']) {
        'conflict' => l.dockerKubeNameTaken,
        'forbidden' || 'unauthorized' => l.kubeApplyForbidden,
        'invalid_request' || 'rejected' => l.kubeApplyInvalid,
        _ => l.dockerKubeFailed,
      };
    }
    return l.dockerKubeFailed;
  }

  Future<void> _status({int remaining = 24}) async {
    if (!_current || !_started || _readingStatus) return;
    _statusTimer?.cancel();
    final generation = _generation;
    bool current() => _current && generation == _generation;
    setState(() => _readingStatus = true);
    try {
      final result = await _deployment!.readRollout(_service as KubeBrowserService, current: current);
      if (!current()) return;
      _rollout = result;
      if (result.isProcessOrImageFailure) _lastPodProblem = result;
      if (result.state == DockerKubeRolloutState.ready || result.state == DockerKubeRolloutState.replaced) {
        _lastPodProblem = null;
      }
      if (remaining > 0 && (result.state == DockerKubeRolloutState.progressing || result.isProcessOrImageFailure)) {
        _statusTimer = Timer(const Duration(seconds: 5), () => unawaited(_status(remaining: remaining - 1)));
      }
    } catch (_) {
      if (current()) _rollout = const DockerKubeRollout(DockerKubeRolloutState.unavailable);
    } finally {
      if (current()) setState(() => _readingStatus = false);
    }
  }

  String _statusLabel(AppLocalizations l, DockerKubeRollout rollout) {
    final message = switch (rollout.state) {
      DockerKubeRolloutState.ready => l.dockerKubeReady,
      DockerKubeRolloutState.progressing => l.dockerKubePending,
      DockerKubeRolloutState.imagePull => l.kubeIssueImagePull,
      DockerKubeRolloutState.crashLoop => l.kubeIssueCrashLoop,
      DockerKubeRolloutState.terminated => l.dockerKubeTerminated,
      DockerKubeRolloutState.unschedulable => l.dockerKubeUnschedulable,
      DockerKubeRolloutState.containerConfig => l.dockerKubeContainerConfig,
      DockerKubeRolloutState.deadlineExceeded => l.dockerKubeDeadlineExceeded,
      DockerKubeRolloutState.replicaFailure => l.dockerKubeReplicaFailure,
      DockerKubeRolloutState.unavailable => l.dockerKubeStatusUnavailable,
      DockerKubeRolloutState.replaced => l.dockerKubeReplaced,
    };
    return [
      message,
      ?rollout.reason,
      ?rollout.terminationReason,
      if (rollout.exitCode case final code?) l.dockerKubeExitCode(code),
      if (rollout.restarts case final count? when count > 0) '${l.dockerRestartCount} · $count',
    ].join(' · ');
  }

  void _openDeployment(KubeResourceRef resource) {
    if (!_current || _busy || _context == null || _rollout?.state == DockerKubeRolloutState.replaced) return;
    closeDialog<KubeNavigationRequest>(
      context,
      KubeNavigationRequest(scope: _scope, contextId: _context!.id, namespace: resource.namespace, resource: resource),
    );
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
                  DockerKubeBlocker.startup => l.dockerKubeStartupUnavailable,
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
    if (_review case final review? when _current) {
      return GlassDialog(
        title: '${l.dockerKubeGenerate} · ${review.provider.name}',
        width: 820,
        content: SizedBox(
          height: 360,
          child: ListView(
            padding: EdgeInsets.zero,
            children: [
              Text(l.dockerKubeAiReview),
              const SizedBox(height: GlassSpacing.s8),
              SizedBox(
                height: 260,
                child: TextFormField(
                  key: const ValueKey('docker-kube-prompt'),
                  initialValue: review.prompt,
                  expands: true,
                  minLines: null,
                  maxLines: null,
                  textAlignVertical: TextAlignVertical.top,
                  autocorrect: false,
                  enableSuggestions: false,
                  enableIMEPersonalizedLearning: false,
                  decoration: InputDecoration(labelText: l.dockerKubePrompt, alignLabelWithHint: true),
                  onChanged: (text) => setState(() {
                    _error = null;
                    _review = (provider: review.provider, target: review.target, prompt: text);
                  }),
                ),
              ),
              if (_error != null) Text(_error!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            ],
          ),
        ),
        primaryAction: GlassButton.prominent(
          key: const ValueKey('docker-kube-send-ai'),
          label: l.dockerKubeSendAi,
          onPressed: review.prompt.trim().isEmpty ? null : _generate,
        ),
        secondaryActions: [
          GlassButton(
            label: l.commonCancel,
            onPressed: () => setState(() {
              _review = null;
              _error = null;
            }),
          ),
        ],
      );
    }
    if (_current && _started) {
      final deployment = _deployment!.created.where((r) => r.api.id == DockerKubeParser.deploymentKind.id).firstOrNull;
      final status = _rollout;
      final previous = status?.state == DockerKubeRolloutState.progressing ? _lastPodProblem : null;
      final problem = status?.isProcessOrImageFailure == true ? status : previous;
      return PopScope(
        canPop: !_busy,
        child: GlassDialog(
          title: l.dockerKubeDeploy,
          width: 660,
          content: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: GlassSpacing.s8,
              children: [
                Text(l.dockerKubeCreated),
                for (final resource in _deployment!.created)
                  SelectableText('${resource.api.kind} ${resource.namespace}/${resource.name}'),
                if (_error != null) Text(_error!, key: const ValueKey('docker-kube-error')),
                if (status != null) Text(_statusLabel(l, status), key: const ValueKey('docker-kube-status')),
                if (previous != null) Text('${l.dockerKubeLastProblem} · ${_statusLabel(l, previous)}'),
                if (problem case final result?) ...[
                  if (result.image case final image?)
                    SelectableText('${l.dockerImage} · $image')
                  else if (_target.manifestImage case final requestedImage?)
                    SelectableText('${l.dockerKubeRequestedImage} · $requestedImage'),
                  Text(switch (result) {
                    DockerKubeRollout(registryDenied: true) => l.dockerKubeRegistryDenied,
                    DockerKubeRollout(state: DockerKubeRolloutState.imagePull) => l.dockerKubePullAdvice,
                    DockerKubeRollout(terminationReason: 'OOMKilled') => l.dockerKubeOomAdvice,
                    DockerKubeRollout(exitCode: 0) ||
                    DockerKubeRollout(terminationReason: 'Completed') => l.dockerKubeCompletionAdvice,
                    _ => l.dockerKubeCrashAdvice,
                  }),
                ],
                if (_rollout?.podDetailsUnavailable ?? false) Text(l.dockerKubePodDetailsUnavailable),
                if (_readingStatus || _busy) const LinearProgressIndicator(),
                if (!_busy && deployment != null && _rollout?.state != DockerKubeRolloutState.replaced)
                  GlassButton(
                    key: const ValueKey('docker-kube-open-deployment'),
                    label: l.dockerKubeOpenDeployment,
                    icon: Icons.open_in_new_rounded,
                    onPressed: () => _openDeployment(deployment),
                  ),
              ],
            ),
          ),
          primaryAction: GlassButton(label: l.commonClose, onPressed: _busy ? null : () => closeDialog<void>(context)),
          secondaryActions: [
            GlassButton(label: l.dockerKubeCheckStatus, onPressed: _busy || _readingStatus ? null : () => _status()),
          ],
        ),
      );
    }
    final enabled = !_busy && !_started;
    final reviewControls = <Widget>[
      const Divider(height: 12),
      if (_error != null)
        Text(
          _error!,
          key: const ValueKey('docker-kube-error'),
          style: TextStyle(color: Theme.of(context).colorScheme.error),
        ),
      CheckboxListTile(
        key: const ValueKey('docker-kube-acknowledge'),
        dense: true,
        contentPadding: EdgeInsets.zero,
        controlAffinity: ListTileControlAffinity.leading,
        title: Text(l.dockerKubeAcknowledge),
        value: _acknowledged,
        onChanged: enabled
            ? (value) {
                _acknowledged = value ?? false;
                _changed();
              }
            : null,
      ),
      if (_deployment?.validated ?? false) Text(l.dockerKubeValidated, key: const ValueKey('docker-kube-validated')),
    ];
    Widget field(TextEditingController controller, String key, String label) => TextField(
      key: ValueKey('docker-kube-$key'),
      controller: controller,
      enabled: enabled,
      autocorrect: false,
      enableSuggestions: false,
      enableIMEPersonalizedLearning: false,
      decoration: InputDecoration(labelText: label),
      onChanged: (_) {
        if (identical(controller, _image)) {
          _imageEdited = true;
          _pinnedImage = null;
        }
        _changed(target: true);
      },
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
              : LayoutBuilder(
                  builder: (context, bounds) {
                    final scrollReview = bounds.maxHeight < 340;
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        SizedBox(height: 2, child: _busy ? const LinearProgressIndicator(minHeight: 2) : null),
                        Expanded(
                          child: Scrollbar(
                            controller: _formScroll,
                            thumbVisibility: true,
                            child: SingleChildScrollView(
                              controller: _formScroll,
                              padding: const EdgeInsetsDirectional.only(top: 8, end: 16, bottom: 16),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.stretch,
                                spacing: GlassSpacing.s16,
                                children: [
                                  Text(l.dockerKubeIntro, style: Theme.of(context).textTheme.bodySmall),
                                  DropdownButtonFormField<String>(
                                    key: const ValueKey('docker-kube-context'),
                                    initialValue: _context?.id,
                                    isExpanded: true,
                                    decoration: InputDecoration(labelText: l.dockerKubeContext),
                                    items: [
                                      for (final c in _contexts) DropdownMenuItem(value: c.id, child: Text(c.name)),
                                    ],
                                    onChanged: enabled
                                        ? (id) {
                                            if (id != null) {
                                              unawaited(_connect(_contexts.firstWhere((c) => c.id == id)));
                                            }
                                          }
                                        : null,
                                  ),
                                  if (session != null) ...[
                                    KubeWriteBanner(session: session, contextName: _context!.name),
                                    if (!_newNamespace)
                                      DropdownButtonFormField<String>(
                                        key: ValueKey('docker-kube-namespace-${session.id}-${_namespace.text}'),
                                        initialValue: _namespaces.contains(_namespace.text) ? _namespace.text : null,
                                        isExpanded: true,
                                        decoration: InputDecoration(labelText: l.kubeNamespace),
                                        items: [
                                          for (final ns in _namespaces) DropdownMenuItem(value: ns, child: Text(ns)),
                                        ],
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
                                          onPressed:
                                              enabled && _canWrite && DockerKubeTarget.validName(_namespace.text.trim())
                                              ? _createNamespace
                                              : null,
                                        ),
                                      ),
                                    ],
                                    if (_namespaceCreated)
                                      Text(
                                        l.dockerKubeNamespaceCreated,
                                        key: const ValueKey('docker-kube-namespace-created'),
                                      ),
                                  ],
                                  field(_name, 'name', l.kubeName),
                                  const Divider(height: 8),
                                  if (widget.publishImage != null)
                                    RegistryPublisher(
                                      enabled: enabled,
                                      publishing: _publishing,
                                      engineName: widget.engineName,
                                      sourceImage: _source.sourceImage ?? widget.container.image,
                                      allowBuild: widget.buildImage != null,
                                      remoteBuild: widget.remoteBuild,
                                      publish: _publish,
                                    ),
                                  Row(
                                    children: [
                                      Expanded(child: field(_image, 'image', l.dockerKubeImage)),
                                      const SizedBox(width: 8),
                                      if (DockerKubeTarget.validImage(_image.text.trim()))
                                        GlassIconButton(
                                          tooltip: l.commonCopy,
                                          icon: Icons.copy_rounded,
                                          onPressed: () => copyPlainWithNotice(context, ref, _image.text.trim()),
                                        ),
                                    ],
                                  ),
                                  if (_pinnedImage != null)
                                    CheckboxListTile(
                                      key: const ValueKey('docker-kube-pin'),
                                      dense: true,
                                      contentPadding: EdgeInsets.zero,
                                      controlAffinity: ListTileControlAffinity.leading,
                                      value: _image.text == _pinnedImage,
                                      title: Text(l.registryPin),
                                      onChanged: enabled ? (v) => _selectPin(v == true) : null,
                                    ),
                                  if (_resolvingImage) const LinearProgressIndicator(minHeight: 2),
                                  Text(
                                    !_resolvingImage &&
                                            _source.image == null &&
                                            !_source.acceptsImage(
                                              _image.text.trim(),
                                              pullSecret: _pullSecret.text.trim(),
                                            )
                                        ? l.dockerKubeUnverifiedImage
                                        : l.dockerKubeImageHelp,
                                  ),
                                  const Divider(height: 8),
                                  field(_port, 'port', l.dockerKubePort),
                                  field(_pullSecret, 'pull-secret', l.dockerKubePullSecret),
                                  DropdownButtonFormField<String>(
                                    key: const ValueKey('docker-kube-provider'),
                                    initialValue: selected?.id.value,
                                    isExpanded: true,
                                    decoration: InputDecoration(labelText: l.kubeAiProvider),
                                    items: [
                                      for (final p in providers)
                                        DropdownMenuItem(value: p.id.value, child: Text(p.name)),
                                    ],
                                    onChanged: enabled
                                        ? (value) =>
                                              setState(() => _providerId = value == null ? null : ObjectId(value))
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
                                        onPressed: enabled && selected != null
                                            ? () => setState(() {
                                                final target = _target;
                                                if (target.valid) {
                                                  _error = null;
                                                  _review = (provider: selected, target: target, prompt: target.prompt);
                                                } else {
                                                  _error = l.dockerKubeInvalidTarget;
                                                }
                                              })
                                            : null,
                                      ),
                                      GlassButton(
                                        key: const ValueKey('docker-kube-template'),
                                        label: l.dockerKubeTemplate,
                                        onPressed: enabled ? _template : null,
                                      ),
                                    ],
                                  ),
                                  if (selected == null) Text(l.dockerKubeNoProvider),
                                  TextField(
                                    key: const ValueKey('docker-kube-yaml'),
                                    controller: _yaml,
                                    enabled: enabled,
                                    minLines: 4,
                                    maxLines: 10,
                                    maxLength: 65536,
                                    autocorrect: false,
                                    enableSuggestions: false,
                                    enableIMEPersonalizedLearning: false,
                                    style: GlassTokens.of(context).typography.mono,
                                    decoration: InputDecoration(
                                      labelText: l.dockerKubeManifestYaml,
                                      alignLabelWithHint: true,
                                      counterText: '',
                                    ),
                                    onChanged: (_) => _changed(),
                                  ),
                                  if (scrollReview) ...reviewControls,
                                ],
                              ),
                            ),
                          ),
                        ),
                        if (!scrollReview) ...reviewControls,
                      ],
                    );
                  },
                ),
        ),
        primaryAction: GlassButton.prominent(
          key: const ValueKey('docker-kube-deploy'),
          label: l.dockerKubeDeployAction,
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
