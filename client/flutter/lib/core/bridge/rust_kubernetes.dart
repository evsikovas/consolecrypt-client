import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_preferences_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_mobile.dart';
import 'package:consolecrypt/kubernetes/kube_forward_service.dart';
import 'package:consolecrypt/kubernetes/kube_forward_targets_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison_service.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_watch_service.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_kubectl_service.dart';
import 'package:consolecrypt/kubernetes/kube_logs_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_organization_service.dart';
import 'package:consolecrypt/kubernetes/kube_relations_service.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_view_preferences_service.dart';
import 'package:consolecrypt/src/rust/api/kubernetes.dart' as native;

final class RustKubeService
    implements
        KubeService,
        KubeRolloutService,
        KubeRelationsService,
        KubeOrganizationService,
        KubeAiService,
        KubeAiHistoryService,
        KubeAiChatService,
        KubeViewPreferencesService,
        KubeInventoryManagementService,
        KubeHistoryService,
        KubeManifestService,
        KubeBrowserService,
        KubeContextTerminalService,
        KubeWatchService,
        KubeMutationService,
        KubeLogService,
        KubeAdvancedLogService,
        KubeInsightsService,
        KubeInsightsWatchService,
        KubeAuthService,
        KubeImportExecService,
        KubeFluxCliService,
        KubeKubectlService,
        KubeFluxPreferencesService,
        KubeHelmCliService,
        KubeHelmComparisonService,
        KubeForwardService,
        KubeNetworkForwardService,
        KubeForwardTargetsService,
        KubeOperationsService,
        KubeTableService,
        KubeMetricsService {
  const RustKubeService(this._hub);
  final RustBackend _hub;

  @override
  Future<KubeHelmComparison> compareHelmLive(String sessionId, String namespace, String secretName) async =>
      KubeHelmComparison.fromJson(
        decodeObject(
          await guard(
            () => native.kubeHelmCompareLive(sessionId: sessionId, namespace: namespace, secretName: secretName),
          ),
        ),
      );

  @override
  Future<KubectlDiscovery> discoverKubectl() async =>
      KubectlDiscovery.fromJson(decodeObject(await guard(native.kubeKubectlDiscover)));
  @override
  Future<FluxCliVersion> probeKubectl(String path) async =>
      FluxCliVersion.fromJson(decodeObject(await guard(() => native.kubeKubectlProbe(path: path))));
  @override
  Future<FluxInstallReview> reviewKubectl(String method) async =>
      FluxInstallReview.fromJson(decodeObject(await guard(() => native.kubeKubectlReview(method: method))));

  @override
  Future<KubeJson> readKubeViewPreferences(String contextId) async =>
      decodeObject(await guard(() => native.kubeViewPreferences(contextId: contextId)));
  @override
  Future<void> updateKubeViewPreferences(String contextId, {required KubeJson patch}) =>
      guard(() => native.kubeViewPreferencesUpdate(contextId: contextId, patchJson: jsonEncode(patch)));

  @override
  Future<KubeContextTerminalProfiles> contextTerminalProfiles() async =>
      KubeContextTerminalProfiles.fromJson(decodeObject(await guard(native.kubeContextTerminalProfiles)));
  @override
  Future<String> openContextTerminal(String sessionId, String namespace, String profileId) =>
      guard(() => native.kubeContextTerminal(sessionId: sessionId, namespace: namespace, profileId: profileId));

  @override
  Future<KubeForwardTargets> forwardTargets(String sessionId, KubeResourceRef resource) async =>
      KubeForwardTargets.fromJson(
        decodeObject(
          await guard(
            () => native.kubeForwardTargets(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson())),
          ),
        ),
      );

  @override
  Future<void> saveKubeFavourites(String contextId, {required List<String> namespaces, required List<String> kinds}) =>
      guard(() => native.kubeContextFavourites(id: contextId, namespaces: namespaces, kinds: kinds));
  @override
  Future<void> saveKubeClusterLabels(String contextId, {required List<String> tags, String? color}) =>
      guard(() => native.kubeClusterLabels(contextId: contextId, tags: tags, color: color));

  @override
  Future<KubeLogTargets> logTargets(String sessionId, KubeResourceRef resource) async => KubeLogTargets.fromJson(
    decodeObject(
      await guard(() => native.kubeLogTargets(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()))),
    ),
  );
  @override
  Future<String> logsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  }) async =>
      decodeObject(
            await guard(
              () => native.kubeLogs(
                sessionId: sessionId,
                namespace: resource.namespace ?? 'default',
                pod: resource.name,
                queryJson: jsonEncode({
                  'container': container,
                  'previous': previous,
                  'timestamps': timestamps,
                  'tail_lines': tailLines,
                  'since_seconds': sinceSeconds,
                }),
              ),
            ),
          )['text']
          as String;
  @override
  Stream<String> followLogsSince(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 1000,
    int? sinceSeconds,
  }) => _nativeStream(
    start: () => guard(
      () => native.kubeLogsStart(
        sessionId: sessionId,
        namespace: resource.namespace ?? 'default',
        pod: resource.name,
        queryJson: jsonEncode({
          'container': container,
          'previous': previous,
          'timestamps': timestamps,
          'tail_lines': tailLines,
          'since_seconds': sinceSeconds,
        }),
      ),
    ),
    poll: (id) async => decodeObject(await guard(() => native.kubeLogsPoll(sessionId: sessionId, streamId: id))),
    stop: (id) => guard(() => native.kubeLogsStop(sessionId: sessionId, streamId: id)),
    decode: (page) => (page['lines'] as List).cast<String>(),
  );
  @override
  Future<void> exportLogs(String sessionId, String path, Uint8List contents, {required bool mask}) =>
      guard(() => native.kubeExportLogs(sessionId: sessionId, path: path, contents: contents, mask: mask));

  @override
  Future<KubeJson> relatedObjects(String sessionId, KubeResourceRef resource) async => decodeObject(
    await guard(() => native.kubeRelatedObjects(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()))),
  );

  @override
  Future<FluxPreferencesView> fluxPreferences({String? sessionId}) async =>
      FluxPreferencesView.fromJson(decodeObject(await guard(() => native.kubeFluxPreferences(sessionId: sessionId))));
  @override
  Future<void> saveFluxPreferences(FluxPreferences preferences) =>
      guard(() => native.kubeFluxPreferencesSave(preferencesJson: jsonEncode(preferences.toJson())));
  @override
  Future<FluxPathAdvice> addFluxUserPath() async =>
      FluxPathAdvice.fromJson(decodeObject(await guard(native.kubeFluxPathAdd)));

  @override
  Future<List<KubeJson>> rolloutHistory(String sessionId, KubeResourceRef resource) async => (jsonDecode(
    await guard(() => native.kubeRolloutHistory(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()))),
  ) as List).cast<KubeJson>();
  @override
  Future<KubeJson> rolloutDiff(String sessionId, KubeResourceRef resource, int before, int after) async => decodeObject(
    await guard(
      () => native.kubeRolloutDiff(
        sessionId: sessionId,
        referenceJson: jsonEncode(resource.toJson()),
        before: before,
        after: after,
      ),
    ),
  );

  @override
  Future<List<KubeJson>> kubeAiHistory(String contextId) async {
    final bytes = await guard(() => native.kubeAiHistory(contextId: contextId));
    try {
      return (jsonDecode(utf8.decode(bytes)) as List).cast<KubeJson>();
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  @override
  Future<void> deleteKubeAiConversation(String conversationId) =>
      guard(() => native.kubeAiDeleteConversation(conversationId: conversationId));

  @override
  Future<KubeJson> resourceSchema(String sessionId, KubeResourceKind kind) async =>
      decodeObject(await guard(() => native.kubeSchema(sessionId: sessionId, kindJson: jsonEncode(kind.toJson()))));
  @override
  Future<KubeJson> compareManifest(String sessionId, KubeResourceRef resource, SecretText manifest) async =>
      decodeObject(
        await withSecret(
          manifest,
          (bytes) => guard(
            () => native.kubeManifestDiff(
              sessionId: sessionId,
              referenceJson: jsonEncode(resource.toJson()),
              manifest: bytes,
            ),
          ),
        ),
      );
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async => KubeResourceDetail.fromJson(
    decodeObject(
      await withSecret(
        manifest,
        (bytes) => guard(
          () => native.kubeCreateResource(
            sessionId: sessionId,
            referenceJson: jsonEncode(resource.toJson()),
            manifest: bytes,
            dryRun: dryRun,
          ),
        ),
      ),
    ),
  );
  @override
  Future<SecretText> revealSecret(String sessionId, KubeResourceRef resource) async {
    final bytes = await guard(
      () => native.kubeSecretReveal(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson())),
    );
    try {
      return SecretText.fromBytes(bytes);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  @override
  Future<KubeJson> historyStatus(String sessionId) async =>
      decodeObject(await guard(() => native.kubeHistoryStatus(sessionId: sessionId)));

  @override
  Future<List<KubeJson>> eventHistory(
    String contextId, {
    String? namespace,
    bool warningOnly = false,
    int? beforeMs,
    String? beforeUid,
    int limit = 200,
  }) async => (jsonDecode(
    await guard(
      () => native.kubeEventHistory(
        contextId: contextId,
        namespace: namespace,
        warningOnly: warningOnly,
        beforeMs: beforeMs,
        beforeUid: beforeUid,
        limit: limit,
      ),
    ),
  ) as List).cast<KubeJson>();

  @override
  Future<void> clearEventHistory(String contextId) => guard(() => native.kubeEventHistoryClear(contextId: contextId));

  @override
  Future<List<String>> scanConfigFiles(String directory) =>
      guard(() => native.kubeScanConfigFiles(directory: directory));

  @override
  Future<void> exportContext(String contextId, String path, {bool includeCredentials = false}) =>
      guard(() => native.kubeExportContext(contextId: contextId, path: path, includeCredentials: includeCredentials));

  @override
  Future<void> updateContext(
    String contextId, {
    required String name,
    required String namespace,
    required bool production,
    required bool allowExecInProd,
    required bool allowPortForwardInProd,
    required bool recordEvents,
  }) => guard(
    () => native.kubeContextUpdate(
      contextId: contextId,
      updateJson: jsonEncode({
        'name': name,
        'namespace': namespace,
        'production': production,
        'allow_exec_in_prod': allowExecInProd,
        'allow_port_forward_in_prod': allowPortForwardInProd,
        'record_events': recordEvents,
      }),
    ),
  );

  @override
  Future<KubeAiReview> prepareKubeAi(
    String sessionId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) => _prepareKubeAi(
    sessionId,
    providerId: providerId,
    question: question,
    selected: selected,
    configMapValues: configMapValues,
  );

  @override
  Future<KubeAiReview> prepareKubeFollowUp(
    String sessionId,
    String conversationId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) => _prepareKubeAi(
    sessionId,
    conversationId: conversationId,
    providerId: providerId,
    question: question,
    selected: selected,
    configMapValues: configMapValues,
  );

  Future<KubeAiReview> _prepareKubeAi(
    String sessionId, {
    String? conversationId,
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) async {
    final payload = SecretText(
      jsonEncode({
        'provider_id': providerId,
        'question': question,
        'selected': selected?.toJson(),
        'config_map_values': configMapValues,
        'conversation_id': conversationId,
      }),
    );
    try {
      return KubeAiReview.fromJson(
        decodeObject(
          await withSecret(
            payload,
            (bytes) => guard(() => native.kubeAiPrepare(sessionId: sessionId, questionJson: bytes)),
          ),
        ),
      );
    } finally {
      payload.wipe();
    }
  }

  @override
  Stream<KubeAiEvent> askKubeAi(
    String sessionId,
    KubeAiReview review, {
    required bool approveExternal,
  }) => _nativeStream(
    start: () =>
        guard(() => native.kubeAiStart(sessionId: sessionId, ticket: review.ticket, approveExternal: approveExternal)),
    poll: (id) async {
      final result = KubeAiPoll.fromBytes(await guard(() => native.kubeAiPoll(sessionId: sessionId, requestId: id)));
      return {'closed': result.closed, 'events': result.events};
    },
    stop: (id) => guard(() => native.kubeAiCancel(sessionId: sessionId, id: id)),
    decode: (page) => (page['events'] as List).cast<KubeAiEvent>(),
  );

  @override
  Future<void> discardKubeAi(String sessionId, String ticket) =>
      guard(() => native.kubeAiCancel(sessionId: sessionId, id: ticket));

  @override
  Future<void> revokeKubeAi(String sessionId) => guard(() => native.kubeAiRevoke(sessionId: sessionId));

  @override
  Future<FluxCliDiscovery> discoverFlux({String? manualPath}) async =>
      FluxCliDiscovery.fromJson(decodeObject(await guard(() => native.kubeFluxDiscover(manualPath: manualPath))));
  @override
  Future<FluxCliVersion> probeFlux(String path) async =>
      FluxCliVersion.fromJson(decodeObject(await guard(() => native.kubeFluxProbe(path: path))));
  @override
  Future<FluxInstallReview> reviewFlux(String version, String method, String operation) async =>
      FluxInstallReview.fromJson(
        decodeObject(await guard(() => native.kubeFluxReview(version: version, method: method, operation: operation))),
      );
  @override
  Future<String?> approveFlux(FluxInstallReview review) => guard(() => native.kubeFluxApprove(ticket: review.ticket));
  @override
  Future<void> cancelFlux(String ticket) => guard(() => native.kubeFluxCancel(ticket: ticket));
  @override
  Future<FluxCliOutput> runFlux(String sessionId, String path, KubeJson action) async => FluxCliOutput.fromBytes(
    await guard(() => native.kubeFluxRun(sessionId: sessionId, path: path, actionJson: jsonEncode(action))),
  );
  @override
  Future<String> openFluxTerminal(String sessionId, String path) =>
      guard(() => native.kubeFluxTerminal(sessionId: sessionId, path: path));

  @override
  Future<HelmRollbackReview> reviewHelmRollback(
    String sessionId,
    String namespace,
    String release,
    int revision, {
    String? manualPath,
  }) async => HelmRollbackReview.fromJson(
    decodeObject(
      await guard(
        () => native.kubeHelmRollbackReview(
          sessionId: sessionId,
          namespace: namespace,
          release: release,
          revision: revision,
          manualPath: manualPath,
        ),
      ),
    ),
  );
  @override
  Future<FluxCliOutput> approveHelmRollback(String ticket) async =>
      FluxCliOutput.fromBytes(await guard(() => native.kubeHelmRollbackApprove(ticket: ticket)));
  @override
  Future<void> cancelHelmRollback(String ticket) => guard(() => native.kubeFluxCancel(ticket: ticket));

  @override
  Stream<void> changes() => Stream<void>.multi((controller) {
    void changed(Json event) {
      if (const {
        'objects_changed',
        'conflict_resolved',
        'lagged',
        'vault_unlocked',
        'vault_locked',
        'profiles_changed',
      }.contains(event['type'])) {
        controller.add(null);
      }
    }

    _hub.eventListeners.add(changed);
    controller.add(null);
    controller.onCancel = () => _hub.eventListeners.remove(changed);
  }, isBroadcast: true);

  @override
  Future<List<KubeConnection>> inventory() async {
    final data = decodeObject(await guard(native.kubeInventory));
    final clusters = {for (final c in (data['clusters'] as List).cast<Json>()) c['id'] as String: c};
    return List.unmodifiable(
      (data['contexts'] as List).cast<Json>().map(
        (context) => KubeConnection.fromJson(context, clusters[context['cluster_id']]),
      ),
    );
  }

  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) async => KubeImportPreview.fromJson(
    decodeObject(
      await withSecret(yaml, (bytes) => guard(() => native.kubeImportPreview(yaml: bytes, sourceFile: sourceFile))),
    ),
  );

  @override
  Future<KubeImportPreview> previewFile(String path) async =>
      KubeImportPreview.fromJson(decodeObject(await guard(() => native.kubeImportPreviewFile(path: path))));

  @override
  Future<void> discard(String ticket) => guard(() => native.kubeImportDiscard(ticket: ticket));

  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async {
    await guard(
      () => native.kubeImportCommit(ticket: ticket, choicesJson: jsonEncode(choices.map((c) => c.toJson()).toList())),
    );
  }

  @override
  Future<void> deleteContext(String id) => guard(() => native.kubeContextDelete(id: id));

  @override
  Future<KubeSession> connect(String contextId) async =>
      KubeSession.fromJson(decodeObject(await guard(() => native.kubeConnect(contextId: contextId))));

  @override
  Future<void> disconnect(String sessionId) => guard(() => native.kubeDisconnect(sessionId: sessionId));

  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async {
    final result = decodeObject(await guard(() => native.kubeDiscover(sessionId: sessionId)));
    return List.unmodifiable((result['resources'] as List).map((e) => KubeResourceKind.fromJson(e as Json)));
  }

  @override
  Future<List<String>> namespaces(String sessionId) async => List<String>.unmodifiable(
    (jsonDecode(await guard(() => native.kubeNamespaces(sessionId: sessionId))) as List).cast<String>(),
  );

  Json _query({String? namespace, String? labelSelector, String? continueToken, int limit = 200}) => {
    'namespace': namespace,
    'label_selector': labelSelector,
    'continue_token': continueToken,
    'limit': limit,
  };

  @override
  Future<KubeResourcePage> listResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  }) async => KubeResourcePage.fromJson(
    decodeObject(
      await guard(
        () => native.kubeResourcesList(
          sessionId: sessionId,
          kindJson: jsonEncode(kind.toJson()),
          queryJson: jsonEncode(
            _query(namespace: namespace, labelSelector: labelSelector, continueToken: continueToken, limit: limit),
          ),
        ),
      ),
    ),
  );

  @override
  Future<KubeResourceDetail> getResource(String sessionId, KubeResourceRef resource) async =>
      KubeResourceDetail.fromJson(
        decodeObject(
          await guard(() => native.kubeResourceGet(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()))),
        ),
      );

  @override
  Future<String> logs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) async =>
      decodeObject(
            await guard(
              () => native.kubeLogs(
                sessionId: sessionId,
                namespace: resource.namespace ?? 'default',
                pod: resource.name,
                queryJson: jsonEncode({
                  'container': container,
                  'previous': previous,
                  'timestamps': timestamps,
                  'tail_lines': tailLines,
                }),
              ),
            ),
          )['text']
          as String;

  @override
  Stream<KubeWatchEvent> watchResources(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    required String resourceVersion,
  }) => _nativeStream(
    start: () => guard(
      () => native.kubeWatchStart(
        sessionId: sessionId,
        kindJson: jsonEncode(kind.toJson()),
        queryJson: jsonEncode(_query(namespace: namespace, labelSelector: labelSelector)),
        resourceVersion: resourceVersion.isEmpty ? null : resourceVersion,
      ),
    ),
    poll: (id) async => decodeObject(await guard(() => native.kubeWatchPoll(sessionId: sessionId, watchId: id))),
    stop: (id) => guard(() => native.kubeWatchStop(sessionId: sessionId, watchId: id)),
    decode: (page) => (page['events'] as List).map((event) => KubeWatchEvent.fromJson(event as Json)),
  );

  @override
  Stream<String> followLogs(
    String sessionId,
    KubeResourceRef resource, {
    String? container,
    bool previous = false,
    bool timestamps = true,
    int tailLines = 500,
  }) => _nativeStream(
    start: () => guard(
      () => native.kubeLogsStart(
        sessionId: sessionId,
        namespace: resource.namespace ?? 'default',
        pod: resource.name,
        queryJson: jsonEncode({
          'container': container,
          'previous': previous,
          'timestamps': timestamps,
          'tail_lines': tailLines,
        }),
      ),
    ),
    poll: (id) async => decodeObject(await guard(() => native.kubeLogsPoll(sessionId: sessionId, streamId: id))),
    stop: (id) => guard(() => native.kubeLogsStop(sessionId: sessionId, streamId: id)),
    decode: (page) => (page['lines'] as List).cast<String>(),
  );

  Stream<T> _nativeStream<T>({
    required Future<String> Function() start,
    required Future<Json> Function(String) poll,
    required Future<void> Function(String) stop,
    required Iterable<T> Function(Json) decode,
  }) {
    late final StreamController<T> controller;
    bool active = true;
    String? handle;
    Completer<void>? paused;
    Future<void> closeNative() async {
      final id = handle;
      if (id != null) {
        try {
          await stop(id);
        } catch (_) {}
      }
    }

    Future<void> run() async {
      try {
        handle = await start();
        while (active) {
          if (paused != null) await paused!.future;
          if (!active) break;
          final page = await poll(handle!);
          if (!active) break;
          if (paused != null) await paused!.future;
          if (!active) break;
          for (final event in decode(page)) {
            controller.add(event);
          }
          if (page['closed'] == true) break;
        }
      } catch (error, stack) {
        if (active) controller.addError(error, stack);
      } finally {
        await closeNative();
        unawaited(controller.close());
      }
    }

    controller = StreamController<T>(
      onListen: () => unawaited(run()),
      onPause: () => paused ??= Completer<void>(),
      onResume: () {
        paused?.complete();
        paused = null;
      },
      onCancel: () {
        active = false;
        paused?.complete();
        paused = null;
        return closeNative();
      },
    );
    return controller.stream;
  }

  @override
  Future<KubeInsights> insights(String sessionId, {String? namespace}) async => KubeInsights.fromJson(
    decodeObject(await guard(() => native.kubeInsights(sessionId: sessionId, namespace: namespace))),
  );

  @override
  Stream<KubeInsightsInvalidation> watchInsights(String sessionId, {String? namespace}) {
    bool? previousPartial;
    return _nativeStream(
      start: () async {
        final result = decodeObject(
          await guard(() => native.kubeInsightsWatchStart(sessionId: sessionId, namespace: namespace)),
        );
        return result['id'] as String;
      },
      poll: (id) async =>
          decodeObject(await guard(() => native.kubeInsightsWatchPoll(sessionId: sessionId, watchId: id))),
      stop: (id) => guard(() => native.kubeInsightsWatchStop(sessionId: sessionId, watchId: id)),
      decode: (page) {
        final partial = page['partial'] == true;
        final changed = partial != previousPartial;
        previousPartial = partial;
        return page['dirty'] == true || page['closed'] == true || changed
            ? [KubeInsightsInvalidation(partial: partial, closed: page['closed'] == true)]
            : const <KubeInsightsInvalidation>[];
      },
    );
  }

  @override
  Future<List<KubeHelmSummary>> helmHistory(String sessionId, {String? namespace, String? releaseName}) async =>
      List.unmodifiable(
        (jsonDecode(
          await guard(
            () => native.kubeHelmHistory(sessionId: sessionId, namespace: namespace, releaseName: releaseName),
          ),
        ) as List).map((value) => KubeHelmSummary.fromJson(value as Json)),
      );

  @override
  Future<KubeHelmRelease> helmRelease(String sessionId, String namespace, String secretName) async =>
      KubeHelmRelease.fromJson(
        decodeObject(
          await guard(() => native.kubeHelmRelease(sessionId: sessionId, namespace: namespace, secretName: secretName)),
        ),
      );

  @override
  Future<void> fluxAction(String sessionId, KubeResourceRef resource, KubeFluxAction action, {String? reason}) => guard(
    () => native.kubeFluxAction(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      reason: reason,
      action: switch (action) {
        KubeFluxAction.reconcileWithSource => 'reconcile_with_source',
        KubeFluxAction.resetFailures => 'reset_failures',
        KubeFluxAction.forceUpgrade => 'force_upgrade',
        _ => action.name,
      },
    ),
  );

  @override
  Future<void> argoAction(String sessionId, KubeResourceRef resource, KubeArgoAction action) => guard(
    () => native.kubeArgoAction(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      action: action == KubeArgoAction.hardRefresh ? 'hard_refresh' : action.name,
    ),
  );

  @override
  Future<KubeWriteAccess> writeAccess(String sessionId) async =>
      KubeWriteAccess.fromJson(decodeObject(await guard(() => native.kubeWriteAccess(sessionId: sessionId))));

  @override
  Future<KubeWriteAccess> requestWriteWindow(String sessionId, String typedContextName) async =>
      KubeWriteAccess.fromJson(
        decodeObject(
          await guard(() => native.kubeRequestWriteWindow(sessionId: sessionId, typedContextName: typedContextName)),
        ),
      );

  @override
  Future<void> closeWriteWindow(String sessionId) => guard(() => native.kubeCloseWriteWindow(sessionId: sessionId));

  @override
  Future<KubeResourceDetail> applyYaml(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
    bool force = false,
  }) async => KubeResourceDetail.fromJson(
    decodeObject(
      await withSecret(
        manifest,
        (bytes) => guard(
          () => native.kubeApplyYaml(
            sessionId: sessionId,
            referenceJson: jsonEncode(resource.toJson()),
            manifest: bytes,
            dryRun: dryRun,
            force: force,
          ),
        ),
      ),
    ),
  );

  @override
  Future<void> deleteResource(
    String sessionId,
    KubeResourceRef resource, {
    required String uid,
    required String resourceVersion,
  }) => guard(
    () => native.kubeResourceDelete(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      uid: uid,
      resourceVersion: resourceVersion,
    ),
  );

  @override
  Future<KubeImportExecInspection> inspectImportExec(String ticket, int index) async =>
      KubeImportExecInspection.fromBytes(await guard(() => native.kubeImportInspectExec(ticket: ticket, index: index)));

  @override
  Future<KubeExecReview> reviewExec(String contextId) async =>
      KubeExecReview.fromBytes(await guard(() => native.kubeExecReview(contextId: contextId)));

  @override
  Future<void> approveExec(String ticket) => guard(() => native.kubeExecApprove(ticket: ticket));

  @override
  Future<KubeOidcChallenge> beginOidc(String contextId) async =>
      KubeOidcChallenge.fromBytes(await guard(() => native.kubeOidcBegin(contextId: contextId)));

  @override
  Future<void> completeOidc(String ticket, {String? callbackUrl}) =>
      guard(() => native.kubeOidcComplete(ticket: ticket, callbackUrl: callbackUrl));

  @override
  Future<void> cancelAuth(String ticket) => guard(() => native.kubeAuthCancel(ticket: ticket));

  @override
  Future<KubeForward> startForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    int localPort = 0,
  }) async {
    await KubeForwardMobile.prepare();
    final forward = KubeForward.fromJson(
      decodeObject(
        await guard(
          () => native.kubeForwardStart(
            sessionId: sessionId,
            namespace: namespace,
            pod: pod,
            remotePort: remotePort,
            localPort: localPort,
          ),
        ),
      ),
    );
    try {
      await KubeForwardMobile.keepVisible();
      return forward;
    } catch (_) {
      await stopForward(sessionId, forward.id);
      rethrow;
    }
  }

  @override
  Future<KubeForward> startNetworkForward(
    String sessionId, {
    required String namespace,
    required String pod,
    required int remotePort,
    required String localAddress,
    required bool approveNetwork,
    int localPort = 0,
  }) async => KubeForward.fromJson(
    decodeObject(
      await guard(
        () => native.kubeForwardStartConfigured(
          sessionId: sessionId,
          requestJson: jsonEncode({
            'namespace': namespace,
            'pod': pod,
            'remote_port': remotePort,
            'local_port': localPort,
            'local_address': localAddress,
            'approve_network': approveNetwork,
          }),
        ),
      ),
    ),
  );

  @override
  Future<List<KubeForward>> forwards() async => List.unmodifiable(
    (jsonDecode(await guard(native.kubeForwardList)) as List).map((value) => KubeForward.fromJson(value as Json)),
  );

  @override
  Future<void> stopForward(String sessionId, String forwardId) =>
      guard(() => native.kubeForwardStop(sessionId: sessionId, forwardId: forwardId));

  @override
  Future<KubeResourceTable> table(
    String sessionId,
    KubeResourceKind kind, {
    String? namespace,
    String? labelSelector,
    String? continueToken,
    int limit = 200,
  }) async => KubeResourceTable.fromJson(
    decodeObject(
      await guard(
        () => native.kubeResourcesTable(
          sessionId: sessionId,
          kindJson: jsonEncode(kind.toJson()),
          queryJson: jsonEncode(
            _query(namespace: namespace, labelSelector: labelSelector, continueToken: continueToken, limit: limit),
          ),
        ),
      ),
    ),
  );

  @override
  Future<KubePermissionReport> permissions(String sessionId, String namespace) async => KubePermissionReport.fromJson(
    decodeObject(await guard(() => native.kubePermissions(sessionId: sessionId, namespace: namespace))),
  );

  @override
  Future<List<KubeMetrics>> metrics(String sessionId, {required bool nodes, String? namespace}) async =>
      List.unmodifiable(
        (jsonDecode(
          await guard(() => native.kubeMetrics(sessionId: sessionId, pods: !nodes, namespace: namespace)),
        ) as List).map((value) => KubeMetrics.fromJson(value as Json)),
      );

  @override
  Future<void> scale(String sessionId, KubeResourceRef resource, String resourceVersion, int replicas) => guard(
    () => native.kubeScale(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      resourceVersion: resourceVersion,
      replicas: replicas,
    ),
  );

  @override
  Future<void> restart(String sessionId, KubeResourceRef resource, String resourceVersion) => guard(
    () => native.kubeRestart(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      resourceVersion: resourceVersion,
    ),
  );

  @override
  Future<void> cordon(String sessionId, String node, String resourceVersion, bool unschedulable) => guard(
    () => native.kubeCordon(
      sessionId: sessionId,
      node: node,
      resourceVersion: resourceVersion,
      unschedulable: unschedulable,
    ),
  );

  @override
  Future<List<KubeDrainCandidate>> drainPlan(String sessionId, String node) async => List.unmodifiable(
    (jsonDecode(await guard(() => native.kubeDrainPlan(sessionId: sessionId, node: node))) as List).map(
      (value) => KubeDrainCandidate.fromJson(value as Json),
    ),
  );

  @override
  Future<void> evictPod(String sessionId, KubeResourceRef resource, String uid, String resourceVersion) => guard(
    () => native.kubeEvictPod(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      uid: uid,
      resourceVersion: resourceVersion,
    ),
  );

  @override
  Future<void> debugContainer(
    String sessionId,
    KubeResourceRef resource,
    String resourceVersion,
    KubeDebugContainer container,
  ) => guard(
    () => native.kubeDebugContainer(
      sessionId: sessionId,
      referenceJson: jsonEncode(resource.toJson()),
      resourceVersion: resourceVersion,
      containerJson: jsonEncode(container.toJson()),
    ),
  );

  @override
  Future<List<KubeResourceRow>> workloadPods(String sessionId, KubeResourceRef resource) async {
    final data = decodeObject(
      await guard(() => native.kubeWorkloadPods(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()))),
    );
    return List.unmodifiable(
      (data['items'] as List).map((value) => KubeResourceRow.fromJson(value as Json, includeObject: true)),
    );
  }

  @override
  Future<List<KubeResourceRow>> objectEvents(String sessionId, KubeResourceRef resource, String uid) async {
    final data = decodeObject(
      await guard(
        () => native.kubeObjectEvents(sessionId: sessionId, referenceJson: jsonEncode(resource.toJson()), uid: uid),
      ),
    );
    return List.unmodifiable(
      (data['items'] as List).map((value) => KubeResourceRow.fromJson(value as Json, includeObject: true)),
    );
  }
}
