import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_review.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation_dialog.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status_review.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter/scheduler.dart';
import 'package:material_ui/material_ui.dart';

/// Explicit read catalog -> prepare exact catalog -> original one-use review.
/// Closing/cancelling cleans the caller channel only, never stops the service.
class AgentCollectorServiceDialog extends StatefulWidget {
  const AgentCollectorServiceDialog({
    super.key,
    required this.port,
    required this.scope,
    required this.record,
    required this.original,
    required this.originalChanges,
    this.elapsed,
  });
  final AgentCollectorServicePort port;
  final RuntimeNoticeScope scope;
  final AgentAssociationRecord record;
  final bool Function() original;
  final Listenable originalChanges;
  final Duration Function()? elapsed;
  @override
  State<AgentCollectorServiceDialog> createState() => _ServiceState();
}

class _ServiceState extends State<AgentCollectorServiceDialog> {
  AgentCollectorServiceController? owner;
  AgentCollectorServiceCatalogView? catalog;
  AgentCollectorServiceStatus? savedStatus;
  Object? statusError;
  bool statusReadSpent = false, readingStatus = false;
  bool savedStatusStale = false, revokeSpent = false, revoking = false;
  AgentCollectorServiceCredentialChoices? credentialChoices;
  bool credentialsReadSpent = false, readingCredentials = false;
  Object? credentialsError;
  String scrapeTargetId = '', scrapeCredentialId = '', receiverCredentialId = '';
  bool lifetimeSelected = false;
  Object? error;
  bool readSpent = false, reading = false, consent = false, retired = false, refreshQueued = false;
  int generation = 0;
  bool get current => mounted && !retired && widget.original();
  @override
  void initState() {
    super.initState();
    widget.originalChanges.addListener(checkOriginal);
  }

  void refresh() {
    if (!mounted) return;
    if (SchedulerBinding.instance.schedulerPhase == SchedulerPhase.persistentCallbacks) {
      if (!refreshQueued) {
        refreshQueued = true;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          refreshQueued = false;
          if (mounted) setState(() {});
        });
      }
    } else {
      setState(() {});
    }
  }

  void retire() {
    if (retired) return;
    retired = true;
    generation++;
    owner?.retire();
    refresh();
  }

  void checkOriginal() {
    if (!widget.original()) retire();
  }

  @override
  void didUpdateWidget(covariant AgentCollectorServiceDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.originalChanges, widget.originalChanges)) {
      old.originalChanges.removeListener(checkOriginal);
      widget.originalChanges.addListener(checkOriginal);
    }
    if (!identical(old.port, widget.port) ||
        !old.scope.same(widget.scope) ||
        !identical(old.record, widget.record) ||
        !widget.original()) {
      retire();
    }
  }

  Future<void> readCatalog() async {
    if (!current || readSpent || readingStatus || readingCredentials || savedStatus != null) return;
    readSpent = true;
    reading = true;
    final captured = generation;
    refresh();
    try {
      final value = await widget.port.readCatalog(widget.scope, widget.record);
      if (!current || captured != generation) return;
      if (!widget.scope.same(value.scope) || !identical(widget.record, value.record)) {
        throw const FormatException('Original service catalog changed');
      }
      catalog = value;
    } catch (failure) {
      if (current && captured == generation) error = failure;
    } finally {
      if (current && captured == generation) {
        reading = false;
        refresh();
      }
    }
  }

  Future<void> prepare() async {
    final observed = catalog;
    if (!current ||
        observed == null ||
        observed.cleanupState != 'closedObserved' ||
        owner != null ||
        savedStatus != null ||
        readingStatus ||
        readingCredentials) {
      return;
    }
    final credentials = selectedCredentials();
    if (lifetimeSelected &&
        (credentials.isEmpty || widget.port is! AgentCollectorServiceLifetimePort)) {
      return;
    }
    final captured = widget;
    final created = AgentCollectorServiceController(
      port: captured.port,
      scope: captured.scope,
      intent: AgentCollectorServiceIntent(
        captured.record, observed.catalog, credentials: credentials,
        credentialUse: lifetimeSelected ? AgentCollectorServiceCredentialUse.serviceLifetime :
            AgentCollectorServiceCredentialUse.singleStart,
      ),
      elapsed: captured.elapsed,
      original: () =>
          mounted &&
          !retired &&
          identical(widget.port, captured.port) &&
          identical(widget.record, captured.record) &&
          widget.scope.same(captured.scope) &&
          widget.original(),
    )..addListener(refresh);
    setState(() => owner = created);
    await created.prepare();
  }

  Future<void> readSavedStatus() async {
    final port = widget.port;
    if (!current ||
        statusReadSpent ||
        reading ||
        readingCredentials ||
        revoking ||
        port is! AgentCollectorServiceStatusPort ||
        owner?.busy == true ||
        owner?.awaitingCleanup == true ||
        owner != null && !owner!.confirmSpent) {
      return;
    }
    statusReadSpent = true;
    readingStatus = true;
    final captured = generation;
    refresh();
    try {
      final value = await (port as AgentCollectorServiceStatusPort).readSavedStatus(widget.scope, widget.record);
      if (!current || captured != generation) return;
      if (!widget.scope.same(value.scope) || !identical(value.record, widget.record)) {
        throw const FormatException('Original saved service changed');
      }
      final reviewed = owner?.view;
      if (reviewed != null &&
          (value.serviceId != reviewed.serviceId ||
              value.serviceHash != reviewed.serviceHash ||
              value.collectorId != reviewed.catalog.collectorId ||
              value.manifestSha256 != reviewed.catalog.manifestSha256)) {
        throw const FormatException('Saved service changed from original review');
      }
      final previous = savedStatus;
      if (previous != null && (value.serviceId != previous.serviceId || value.serviceHash != previous.serviceHash)) {
        throw const FormatException('Original saved service changed');
      }
      savedStatus = value;
      savedStatusStale = false;
      statusError = null;
    } catch (failure) {
      if (current && captured == generation) statusError = failure;
    } finally {
      if (current && captured == generation) {
        readingStatus = false;
        refresh();
      }
    }
  }

  List<AgentCollectorServiceCredentialSelection> selectedCredentials() {
    final choices = credentialChoices?.choices ?? const <AgentCollectorServiceCredentialChoice>[];
    AgentCollectorServiceCredentialSelection ref(String id, String? targetId) {
      final choice = choices.singleWhere((c) => c.id == id);
      return AgentCollectorServiceCredentialSelection(
        credentialId: choice.id,
        credentialRevision: choice.revision,
        targetId: targetId,
      );
    }

    return [
      if (scrapeCredentialId.isNotEmpty) ref(scrapeCredentialId, scrapeTargetId),
      if (receiverCredentialId.isNotEmpty) ref(receiverCredentialId, null),
    ];
  }

  Future<void> readCredentials() async {
    final port = widget.port;
    if (!current ||
        owner != null ||
        savedStatus != null ||
        credentialsReadSpent ||
        readingStatus ||
        reading ||
        catalog?.cleanupState != 'closedObserved' ||
        port is! AgentCollectorServiceCredentialsPort) {
      return;
    }
    credentialsReadSpent = true;
    readingCredentials = true;
    final captured = generation;
    refresh();
    try {
      final choices = await (port as AgentCollectorServiceCredentialsPort).readCredentialChoices(widget.scope);
      if (!current || captured != generation) return;
      if (!widget.scope.same(choices.scope)) throw const FormatException('Original credential choices changed');
      credentialChoices = choices;
      scrapeTargetId = catalog!.catalog.manifest.targets.first.id;
    } catch (failure) {
      if (current && captured == generation) credentialsError = failure;
    } finally {
      if (current && captured == generation) {
        readingCredentials = false;
        refresh();
      }
    }
  }

  Future<void> revokeSavedService() async {
    final target = savedStatus, port = widget.port;
    if (!current ||
        target == null ||
        target.remoteRevoked ||
        savedStatusStale ||
        revokeSpent ||
        readingStatus ||
        port is! AgentCollectorServiceRevocationPort) {
      return;
    }
    revokeSpent = true;
    revoking = true;
    final captured = generation;
    refresh();
    try {
      await showDialog<void>(
        context: context,
        builder: (_) => AgentCollectorServiceRevocationDialog(
          port: port as AgentCollectorServiceRevocationPort,
          target: target,
          originalChanges: widget.originalChanges,
          elapsed: widget.elapsed,
          original: () => current && captured == generation && identical(savedStatus, target),
        ),
      );
    } finally {
      if (current && captured == generation) {
        // Never infer an updated remote/local state from closing the child.
        // Keep the exact target to prohibit creating a replacement policy.
        revoking = false;
        savedStatusStale = true;
        statusReadSpent = false;
        refresh();
      }
    }
  }

  @override
  void dispose() {
    retired = true;
    generation++;
    widget.originalChanges.removeListener(checkOriginal);
    owner?.removeListener(refresh);
    owner?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, o = owner, v = o?.view;
    final lifetime = v?.lifetimeCredentials;
    final cancelAvailable = current && o != null && o.prepareSpent && !o.cancelRequested && (!o.confirmSpent || o.busy);
    Widget credentialPicker(String key, String label, String value, ValueChanged<String> changed) =>
        DropdownButtonFormField<String>(
          key: ValueKey(key),
          initialValue: value,
          isExpanded: true,
          decoration: InputDecoration(labelText: label),
          items: [
            DropdownMenuItem(value: '', child: Text(l.agentCollectorServiceCredentialNone)),
            for (final choice in credentialChoices!.choices)
              DropdownMenuItem(
                value: choice.id,
                child: Tooltip(
                  message: choice.name,
                  child: Text(choice.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                ),
              ),
          ],
          onChanged: current && o == null && !readingStatus
              ? (value) {
                  if (value != null) {
                    setState(() {
                      changed(value);
                      if (scrapeCredentialId.isEmpty && receiverCredentialId.isEmpty) lifetimeSelected = false;
                    });
                  }
                }
              : null,
        );
    return AlertDialog(
      title: Text(l.agentCollectorServiceTitle),
      content: SizedBox(
        width: 600,
        child: SingleChildScrollView(
          child: !current
              ? Text(l.agentCollectorServiceOriginalChanged)
              : Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    if (savedStatus != null && !savedStatusStale)
                      AgentCollectorServiceStatusReview(status: savedStatus!),
                    if (savedStatusStale) Text(l.agentCollectorServiceSavedStale),
                    if (statusError != null) Text(l.agentCollectorServiceSavedUnknown),
                    if (v == null && savedStatus == null) ...[
                      Text(l.agentCollectorServiceRuntimeUnknown),
                      Text(l.agentCollectorServiceCloseDisclosure),
                      if (catalog != null) ...[
                        Text(l.agentCollectorServiceCatalogObserved),
                        SelectableText(catalog!.catalog.manifest.receiverUrl),
                        for (final target in catalog!.catalog.manifest.targets) Text('${target.id}: ${target.url}'),
                        if (catalog!.cleanupState != 'closedObserved') Text(l.agentCollectorServiceCleanupUnknown),
                        if (credentialChoices != null) ...[
                          Text(l.agentCollectorServiceCredentialsDisclosure),
                          DropdownButtonFormField<String>(
                            key: const ValueKey('collector-service-credential-target'),
                            initialValue: scrapeTargetId,
                            isExpanded: true,
                            decoration: InputDecoration(labelText: l.agentMetricsTarget),
                            items: [
                              for (final target in catalog!.catalog.manifest.targets)
                                DropdownMenuItem(value: target.id, child: Text(target.id)),
                            ],
                            onChanged: o == null && !readingStatus
                                ? (value) {
                                    if (value != null) setState(() => scrapeTargetId = value);
                                  }
                                : null,
                          ),
                          credentialPicker(
                            'collector-service-scrape-credential',
                            l.agentCollectorServiceCredentialScrape,
                            scrapeCredentialId,
                            (value) => scrapeCredentialId = value,
                          ),
                          credentialPicker(
                            'collector-service-receiver-credential',
                            l.agentCollectorServiceCredentialReceiver,
                            receiverCredentialId,
                            (value) => receiverCredentialId = value,
                          ),
                        ],
                        if (credentialChoices != null && widget.port is AgentCollectorServiceLifetimePort &&
                            (scrapeCredentialId.isNotEmpty || receiverCredentialId.isNotEmpty)) ...[
                          CheckboxListTile(
                            key: const ValueKey('collector-service-lifetime-selection'),
                            value: lifetimeSelected,
                            title: Text(l.agentCollectorServiceLifetimeChoice),
                            subtitle: Text(l.agentCollectorServiceLifetimeDisclosure),
                            onChanged: o == null && !readingStatus && !readingCredentials ?
                                (value) => setState(() => lifetimeSelected = value ?? false) : null,
                          ),
                        ],
                        if (credentialsError != null) Text(l.agentCollectorServiceCredentialsUnavailable),
                      ],
                    ] else if (v != null)
                      AgentCollectorServiceReview(view: v),
                    if (v?.phase == AgentCollectorServicePhase.serviceReview &&
                        !o!.confirmSpent &&
                        !o.cancelRequested) ...[
                      Text(l.agentCollectorServiceRemaining(o.remainingSeconds)),
                      CheckboxListTile(
                        key: const ValueKey('collector-service-consent'),
                        value: consent,
                        title: Text(lifetime == null ? l.agentCollectorServiceConsent :
                            l.agentCollectorServiceLifetimeConsent),
                        onChanged: o.canConfirm ? (value) => setState(() => consent = value ?? false) : null,
                      ),
                    ],
                    if (o?.expired ?? false) Text(l.agentCollectorServiceExpired),
                    if (o?.unknown ?? false) Text(l.agentCollectorServiceUnknown),
                    if (o?.awaitingCleanup ?? false) Text(l.agentCollectorServiceJoining),
                    if (o?.cleanupUnknown ?? false) Text(l.agentCollectorServiceCleanupUnknown),
                    if (o?.cleanup?.state == 'closedObserved') Text(l.agentCollectorServiceCallerClosed),
                    if (o?.serviceMayRemainActive ?? false) Text(l.agentCollectorServiceMayRemainActive),
                    if (readSpent && !reading && catalog == null) Text(l.agentCollectorServiceUnavailable),
                    if (error != null || o?.error != null) Text(errorMessage(l, error ?? o!.error!)),
                  ],
                ),
        ),
      ),
      actions: [
        if (current &&
            catalog?.cleanupState == 'closedObserved' &&
            o == null &&
            savedStatus == null &&
            !credentialsReadSpent &&
            widget.port is AgentCollectorServiceCredentialsPort)
          TextButton(
            key: const ValueKey('collector-service-credentials'),
            onPressed: readingStatus ? null : readCredentials,
            child: Text(l.agentCollectorServiceReadCredentials),
          ),
        if (current &&
            widget.port is AgentCollectorServiceStatusPort &&
            !statusReadSpent &&
            !reading &&
            !revoking &&
            (o == null || o.confirmSpent && !o.busy && !o.awaitingCleanup))
          TextButton(
            key: const ValueKey('collector-service-status'),
            onPressed: readSavedStatus,
            child: Text(l.agentCollectorServiceReadSavedStatus),
          ),
        if (current &&
            savedStatus != null &&
            !savedStatus!.remoteRevoked &&
            !savedStatusStale &&
            !revokeSpent &&
            !readingStatus &&
            widget.port is AgentCollectorServiceRevocationPort)
          TextButton(
            key: const ValueKey('collector-service-revoke'),
            onPressed: revokeSavedService,
            child: Text(l.agentCollectorServiceRevokeTitle),
          ),
        if (current && !readSpent && savedStatus == null)
          TextButton(
            key: const ValueKey('collector-service-read'),
            onPressed: readingStatus ? null : readCatalog,
            child: Text(l.agentCollectorServiceReadCatalog),
          ),
        if (current && catalog?.cleanupState == 'closedObserved' && o == null && savedStatus == null)
          TextButton(
            key: const ValueKey('collector-service-prepare'),
            onPressed: readingStatus || readingCredentials ? null : prepare,
            child: Text(l.agentCollectorServicePrepare),
          ),
        if (current && o?.canRefresh == true)
          TextButton(
            key: const ValueKey('collector-service-refresh'),
            onPressed: o!.refreshReview,
            child: Text(l.agentCollectorServiceRefresh),
          ),
        if (current && o?.canConfirm == true)
          FilledButton(
            key: const ValueKey('collector-service-confirm'),
            onPressed: consent ? o!.confirmConfiguration : null,
            child: Text(l.agentCollectorServiceConfirm),
          ),
        if (cancelAvailable)
          TextButton(
            key: const ValueKey('collector-service-cancel'),
            onPressed: o.cancel,
            child: Text(l.agentCollectorServiceCancelLocal),
          ),
        if (!cancelAvailable)
          TextButton(
            key: const ValueKey('collector-service-close'),
            onPressed: () => Navigator.pop(context),
            child: Text(l.commonClose),
          ),
      ],
    );
  }
}
