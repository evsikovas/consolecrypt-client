import 'dart:async';

import 'package:consolecrypt/automation/automation_dependency_dialog.dart'
    show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_diff_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_review_cards.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation.dart';
import 'package:consolecrypt/automation/automation_service_results.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

/// A separate, explicit service purpose. Opening this dialog performs no I/O.
class AutomationServiceDelegationDialog extends StatefulWidget {
  const AutomationServiceDelegationDialog({
    super.key,
    required this.service,
    required this.scope,
    required this.uiScope,
    required this.runnerId,
    required this.runnerRevision,
    required this.runnerName,
    required this.original,
    this.originalLive,
    this.intent,
  });
  final AutomationServiceDelegationService service;
  final ServiceDelegationScope scope;
  final AutomationScope uiScope;
  final ValueListenable<bool>? originalLive;
  final String runnerId, runnerName;
  final int runnerRevision;
  final ServiceDelegationIntent? intent;
  final bool Function() original;
  @override
  State<AutomationServiceDelegationDialog> createState() =>
      _AutomationServiceDelegationDialogState();
}

class _AutomationServiceDelegationDialogState
    extends State<AutomationServiceDelegationDialog> {
  final zone = TextEditingController(text: 'UTC'),
      hour = TextEditingController(text: '12'),
      minute = TextEditingController(text: '00'),
      maxJobs = TextEditingController(text: '1'),
      maxDaily = TextEditingController(text: '1');
  final weekdays = <int>{0, 1, 2, 3, 4, 5, 6};
  int validity = 3600, generation = 0;
  bool busy = false,
      retired = false,
      attempted = false,
      uncertain = false,
      revokeReviewed = false;
  Object? error;
  ServiceDelegationPreview? preview;
  AutomationServiceDelegationService? previewOwner;
  ServiceDelegationStatus? status;
  ServiceRecordedResults? results;
  bool resultsUnavailable = false;
  String? cancelledReview;
  Stopwatch? consentClock;
  String? diffReviewedDigest;
  bool get current => mounted && !retired && widget.original();

  @override
  void initState() {
    super.initState();
    for (final controller in [zone, hour, minute, maxJobs, maxDaily]) {
      controller.addListener(changed);
    }
  }

  @override
  void didUpdateWidget(covariant AutomationServiceDelegationDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.service, widget.service) ||
        !identical(oldWidget.scope, widget.scope) ||
        !identical(oldWidget.intent, widget.intent) ||
        oldWidget.runnerId != widget.runnerId ||
        oldWidget.runnerRevision != widget.runnerRevision) {
      retire();
    }
  }

  void retire() {
    if (retired) return;
    retired = true;
    generation++;
    final previous = preview, owner = previewOwner;
    preview = null;
    previewOwner = null;
    diffReviewedDigest = null;
    status = null;
    results = null;
    resultsUnavailable = false;
    error = null;
    busy = false;
    if (previous != null && owner != null && !attempted) {
      unawaited(discard(previous, owner));
    }
  }

  void changed() {
    generation++;
    results = null;
    resultsUnavailable = false;
    final previous = preview, owner = previewOwner;
    preview = null;
    previewOwner = null;
    diffReviewedDigest = null;
    consentClock?.stop();
    if (previous != null && owner != null && !attempted) {
      unawaited(discard(previous, owner));
    }
    if (mounted) {
      setState(() {
        error = null;
        revokeReviewed = false;
      });
    }
  }

  Future<void> discard(
    ServiceDelegationPreview value,
    AutomationServiceDelegationService owner,
  ) async {
    if (cancelledReview == value.reviewId) return;
    cancelledReview = value.reviewId;
    try {
      await owner.cancelServiceDelegation(value.cancellation);
    } catch (_) {
      // Native retains bounded cleanup/expiry. No rollback or Joined is claimed.
    }
  }

  AutomationJson get statusRequest => {
    ...widget.scope.frame,
    'runnerId': widget.runnerId,
    'expectedRunnerRevision': widget.runnerRevision,
  };

  Future<void> prepare() async {
    if (busy || !current || attempted || uncertain || widget.intent == null) {
      return;
    }
    final intent = widget.intent!, owner = widget.service, attempt = generation;
    final chosenHour = int.tryParse(hour.text),
        chosenMinute = int.tryParse(minute.text),
        jobs = int.tryParse(maxJobs.text),
        daily = int.tryParse(maxDaily.text);
    if (zone.text.isEmpty ||
        zone.text.length > 128 ||
        chosenHour == null ||
        chosenHour < 0 ||
        chosenHour > 23 ||
        chosenMinute == null ||
        chosenMinute < 0 ||
        chosenMinute > 59 ||
        weekdays.isEmpty ||
        jobs == null ||
        jobs < 1 ||
        jobs > 128 ||
        daily == null ||
        daily < 1 ||
        daily > jobs) {
      setState(() => error = FormatException(context.l10n.automationRunFields));
      return;
    }
    final selected = intent.prepare(
      zone: zone.text,
      hour: chosenHour,
      minute: chosenMinute,
      weekdays: weekdays.toList()..sort(),
      validitySeconds: validity,
      maxJobs: jobs,
      maxPerUtcDay: daily,
    );
    setState(() {
      busy = true;
      error = null;
      status = null;
    });
    try {
      // The fixed Native operation captures/checks this frame BEFORE attaching
      // to a service. Do not preconnect via the legacy current-profile API.
      final raw = await owner.prepareServiceDelegation(selected);
      final accepted = ServiceDelegationPreview.parse(raw, intent, selected);
      if (!current ||
          !identical(owner, widget.service) ||
          !identical(widget.intent, intent) ||
          attempt != generation) {
        await discard(accepted, owner);
        return;
      }
      setState(() {
        preview = accepted;
        previewOwner = owner;
        consentClock = Stopwatch()..start();
      });
    } catch (failure) {
      if (current && attempt == generation) setState(() => error = failure);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> confirm() async {
    final accepted = preview, owner = previewOwner;
    if (busy ||
        !current ||
        attempted ||
        uncertain ||
        revokeReviewed ||
        accepted == null ||
        owner == null ||
        accepted.review['git_source'] is Map &&
            diffReviewedDigest !=
                (accepted.review['revision'] as Map)['digest']) {
      return;
    }
    if ((consentClock?.elapsedMilliseconds ?? 60001) >=
        accepted.consentRemainingMs) {
      setState(
        () => error = FormatException(
          context.l10n.automationServiceReviewExpired,
        ),
      );
      await edit();
      return;
    }
    // Spent synchronously. Transport uncertainty can never re-arm this policy.
    attempted = true;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final raw = await owner.confirmServiceDelegation(accepted.confirmation);
      final result = ServiceDelegationStatus.parse(
        raw,
        accepted.intent.scope,
        accepted.intent.runnerId,
      );
      if (!result.enrollmentAcknowledged) {
        throw const FormatException('Service acknowledgement unavailable');
      }
      if (current && identical(owner, widget.service)) {
        setState(() => status = result);
      }
    } catch (_) {
      if (current) {
        setState(() {
          uncertain = true;
          status = null;
        });
      }
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> reviewDiff() async {
    final accepted = preview, owner = previewOwner;
    if (busy ||
        !current ||
        accepted == null ||
        owner is! AutomationDiffService) {
      return;
    }
    final digest = (accepted.review['revision'] as Map)['digest'] as String;
    final result = await showGlassDialog<bool>(
      context,
      builder: (_) => DependencyOriginalGuard(
        original: widget.originalLive,
        builder: (_) => AutomationScopeGuard(
          scope: widget.uiScope,
          builder: (_) => AutomationDiffDialog(
            scope: widget.uiScope,
            service: owner as AutomationDiffService,
            projectId: accepted.intent.run['project_id'] as String,
            digest: digest,
          ),
        ),
      ),
    );
    if (current &&
        identical(preview, accepted) &&
        identical(previewOwner, owner) &&
        result == true) {
      setState(() => diffReviewedDigest = digest);
    }
  }

  Future<void> edit() async {
    if (busy || !current || attempted) return;
    final accepted = preview, owner = previewOwner;
    setState(() {
      preview = null;
      previewOwner = null;
      generation++;
      busy = true;
    });
    if (accepted != null && owner != null) await discard(accepted, owner);
    if (current) setState(() => busy = false);
  }

  Future<void> readResults() async {
    final owner = widget.service;
    if (busy || !current || owner is! AutomationServiceResultsService) return;
    final attempt = generation, scope = widget.scope, runner = widget.runnerId;
    final request = statusRequest;
    setState(() {
      busy = true;
      results = null;
      resultsUnavailable = false;
      error = null;
    });
    try {
      final raw = await (owner as AutomationServiceResultsService)
          .serviceDelegationResults(request);
      final accepted = ServiceRecordedResults.parse(raw, scope, runner);
      if (current &&
          identical(owner, widget.service) &&
          attempt == generation) {
        setState(() => results = accepted);
      }
    } catch (failure) {
      if (current &&
          identical(owner, widget.service) &&
          attempt == generation) {
        setState(() {
          resultsUnavailable = true;
          error = failure;
        });
      }
    } finally {
      if (current &&
          identical(owner, widget.service) &&
          attempt == generation) {
        setState(() => busy = false);
      }
    }
  }

  Future<void> observe({bool revoke = false}) async {
    if (busy || !current) return;
    results = null;
    resultsUnavailable = false;
    final owner = widget.service,
        scope = widget.scope,
        runner = widget.runnerId,
        request = statusRequest,
        attempt = generation;
    final previous = !attempted && revoke ? preview : null,
        previousOwner = previewOwner;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      if (previous != null && previousOwner != null) {
        setState(() {
          preview = null;
          previewOwner = null;
          diffReviewedDigest = null;
        });
        await discard(previous, previousOwner);
        if (!current ||
            !identical(owner, widget.service) ||
            attempt != generation) {
          return;
        }
      }
      // In particular, an offline local block must NOT depend on reconnecting
      // the service or refreshing credentials first. Native owns that boundary.
      final raw = revoke
          ? await owner.revokeServiceDelegation(request)
          : await owner.serviceDelegationStatus(request);
      final result = ServiceDelegationStatus.parse(raw, scope, runner);
      if (current &&
          identical(owner, widget.service) &&
          attempt == generation) {
        setState(() {
          status = result;
          revokeReviewed = false;
        });
      }
    } catch (failure) {
      if (current && attempt == generation) setState(() => error = failure);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    retire();
    consentClock?.stop();
    for (final controller in [zone, hour, minute, maxJobs, maxDaily]) {
      controller.dispose();
    }
    super.dispose();
  }

  Widget input(
    String label,
    TextEditingController controller,
    String key, {
    int limit = 6,
  }) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      key: ValueKey(key),
      controller: controller,
      enabled: !busy && preview == null && !attempted,
      maxLength: limit,
      autocorrect: false,
      enableSuggestions: false,
      decoration: InputDecoration(labelText: label, counterText: ''),
    ),
  );
  String resultFlag(bool? value) => value == null
      ? context.l10n.commonUnknown
      : value
      ? context.l10n.automationConfigYes
      : context.l10n.automationConfigNo;
  String resultState(String value) => switch (value) {
    'succeeded' => context.l10n.automationStateSucceeded,
    'failed' => context.l10n.automationStateFailed,
    'cancelled' => context.l10n.automationStateCancelled,
    'interrupted' => context.l10n.automationStateInterrupted,
    _ => context.l10n.automationStateUnknown,
  };
  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 5),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [Text(label), SelectableText(value)],
    ),
  );
  @override
  Widget build(BuildContext context) {
    if (retired || !widget.original()) return const SizedBox.shrink();
    final l = context.l10n, accepted = preview, observed = status;
    final existing =
        observed != null &&
        (observed.localState != 'absent' || observed.remoteState != 'unbound');
    return AlertDialog(
      title: Text(l.automationServiceScheduleTitle),
      content: SizedBox(
        width: 760,
        height:
            (MediaQuery.sizeOf(context).height -
                    MediaQuery.viewInsetsOf(context).bottom -
                    240)
                .clamp(80.0, 560.0),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(widget.runnerName),
              const SizedBox(height: 12),
              InfoBanner(
                tone: BannerTone.warning,
                message: l.automationServiceContinuationDisclosure,
              ),
              if (widget.intent == null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: InfoBanner(
                    tone: BannerTone.info,
                    message: l.automationServiceRunSelectionRequired,
                  ),
                ),
              if (busy)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: 12),
                  child: LinearProgressIndicator(),
                ),
              if (error != null)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: InfoBanner(
                    tone: BannerTone.danger,
                    message: error is FormatException
                        ? l.automationServiceChanged
                        : errorMessage(l, error!),
                  ),
                ),
              if (uncertain && observed?.enrollmentAcknowledged != true)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 12),
                  child: InfoBanner(
                    key: const ValueKey('service-delivery-unknown'),
                    tone: BannerTone.warning,
                    message: l.automationServiceUnknown,
                  ),
                ),
              if (observed != null) ...[
                const SizedBox(height: 12),
                InfoBanner(
                  key: const ValueKey('service-status'),
                  tone: BannerTone.info,
                  message: observed.localState == 'localBlocked'
                      ? l.automationServiceBlocked
                      : observed.localState == 'deliveryUnknown'
                      ? l.automationServiceUnknown
                      : observed.enrollmentAcknowledged
                      ? l.automationServiceAcknowledged
                      : l.automationServiceDataOnly,
                ),
                fact(l.automationServiceStatus, switch (observed.remoteState) {
                  'unbound' => l.automationServiceUnbound,
                  'pending' => l.automationServicePending,
                  'enrolled' => l.automationServiceEnrolled,
                  'restored' => l.automationServiceRestored,
                  'revocationRecorded' => l.automationServiceRevocationRecorded,
                  _ => l.automationServiceRemoteUnknown,
                }),
                if (observed.expiresUtc != null)
                  fact(
                    l.automationServiceExpiry,
                    observed.expiresUtc!.toIso8601String(),
                  ),
              ],
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  TextButton(
                    key: const ValueKey('service-status-read'),
                    onPressed: busy ? null : observe,
                    child: Text(l.automationServiceReadStatus),
                  ),
                  TextButton(
                    key: const ValueKey('service-revoke'),
                    onPressed: busy
                        ? null
                        : () =>
                              setState(() => revokeReviewed = !revokeReviewed),
                    child: Text(l.automationServiceRevoke),
                  ),
                ],
              ),
              if (widget.service is AutomationServiceResultsService)
                Align(
                  alignment: Alignment.centerLeft,
                  child: TextButton(
                    key: const ValueKey('service-results-read'),
                    onPressed: busy ? null : readResults,
                    child: Text(l.automationServiceResultsRead),
                  ),
                ),
              if (resultsUnavailable)
                InfoBanner(
                  key: const ValueKey('service-results-unavailable'),
                  tone: BannerTone.warning,
                  message: l.automationServiceResultsUnavailable,
                ),
              if (results case final recorded?) ...[
                InfoBanner(
                  tone: BannerTone.info,
                  message: l.automationServiceResultsRecorded,
                ),
                if (recorded.jobs.isEmpty)
                  Text(l.automationServiceResultsEmpty),
                for (final job in recorded.jobs)
                  ExpansionTile(
                    key: ValueKey('service-result-${job.jobId}'),
                    title: Text('${job.jobId} · ${resultState(job.state)}'),
                    children: [
                      if (job.partial)
                        InfoBanner(
                          tone: BannerTone.warning,
                          message: l.automationServiceResultsPartial,
                        ),
                      fact(
                        l.automationServiceResultsRunner,
                        resultState(job.runnerState),
                      ),
                      fact(
                        l.automationServiceResultsTarget,
                        job.targetOutcome == 'known'
                            ? l.automationServiceResultsKnown
                            : l.commonUnknown,
                      ),
                      fact(
                        l.automationServiceResultsDue,
                        job.dueUtc.toIso8601String(),
                      ),
                      fact(
                        l.automationServiceResultsUpdated,
                        job.updatedUtc.toIso8601String(),
                      ),
                      fact(
                        l.automationResultExitCode,
                        job.exitCode?.toString() ?? l.commonUnknown,
                      ),
                      fact(
                        l.automationServiceResultsCancelled,
                        resultFlag(job.cancelRequested),
                      ),
                      fact(
                        l.automationServiceResultsTimedOut,
                        resultFlag(job.timedOut),
                      ),
                    ],
                  ),
              ],
              if (revokeReviewed) ...[
                InfoBanner(
                  tone: BannerTone.warning,
                  message: l.automationServiceRevokeReview(widget.runnerName),
                ),
                Align(
                  alignment: Alignment.centerLeft,
                  child: FilledButton(
                    key: const ValueKey('service-revoke-confirm'),
                    onPressed: busy ? null : () => observe(revoke: true),
                    child: Text(l.automationServiceConfirmRevoke),
                  ),
                ),
              ],
              if (accepted == null &&
                  !attempted &&
                  !uncertain &&
                  !existing &&
                  widget.intent != null) ...[
                const SizedBox(height: 16),
                input(
                  l.automationServiceZone,
                  zone,
                  'service-zone',
                  limit: 128,
                ),
                Row(
                  children: [
                    Expanded(
                      child: input(
                        l.automationServiceHour,
                        hour,
                        'service-hour',
                        limit: 2,
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: input(
                        l.automationServiceMinute,
                        minute,
                        'service-minute',
                        limit: 2,
                      ),
                    ),
                  ],
                ),
                Text(l.automationServiceWeekdays),
                Wrap(
                  spacing: 8,
                  children: [
                    for (var day = 0; day < 7; day++)
                      FilterChip(
                        key: ValueKey('service-weekday-$day'),
                        selected: weekdays.contains(day),
                        label: Text(
                          MaterialLocalizations.of(context)
                              .narrowWeekdays[(day + 1) % 7],
                        ),
                        onSelected: busy
                            ? null
                            : (selected) {
                                setState(() {
                                  selected
                                      ? weekdays.add(day)
                                      : weekdays.remove(day);
                                });
                                changed();
                              },
                      ),
                  ],
                ),
                const SizedBox(height: 12),
                GlassSelect<int>(
                  value: validity,
                  expand: true,
                  semanticLabel: l.automationServiceValidity,
                  items: [
                    for (final value in [3600, 14400, 43200, 86400])
                      GlassSelectItem(
                        value: value,
                        label: l.automationServiceValidityHours(
                          (value ~/ 3600).toString(),
                        ),
                      ),
                  ],
                  onChanged: busy
                      ? null
                      : (value) {
                          setState(() => validity = value);
                          changed();
                        },
                ),
                const SizedBox(height: 12),
                input(l.automationServiceMaxJobs, maxJobs, 'service-max-jobs'),
                input(
                  l.automationServiceMaxDaily,
                  maxDaily,
                  'service-max-daily',
                ),
              ],
              if (accepted != null) ...[
                const SizedBox(height: 16),
                fact(
                  l.automationServiceExpiry,
                  accepted.expiresUtc.toIso8601String(),
                ),
                Text(
                  l.automationServiceConsentWindow(
                    (accepted.consentRemainingMs ~/ 1000).toString(),
                  ),
                ),
                fact(
                  l.automationServiceZone,
                  '${accepted.schedule['zone']} · ${accepted.schedule['hour']}:${accepted.schedule['minute'].toString().padLeft(2, '0')}',
                ),
                fact(
                  l.automationServiceWeekdays,
                  (accepted.schedule['weekdays'] as List)
                      .map(
                        (day) =>
                            MaterialLocalizations.of(context)
                                .narrowWeekdays[((day as int) + 1) % 7],
                      )
                      .join(', '),
                ),
                fact(
                  l.automationServiceMaxJobs,
                  (accepted.policy['quotas'] as Map)['max_jobs'].toString(),
                ),
                fact(
                  l.automationServiceMaxDaily,
                  (accepted.policy['quotas'] as Map)['max_per_utc_day']
                      .toString(),
                ),
                Text(l.automationServiceScheduleSemantics),
                fact(
                  l.automationServiceCredentials,
                  (accepted.policy['credentials'] as List).length.toString(),
                ),
                const SizedBox(height: 12),
                AutomationReviewCards(
                  review: accepted.review,
                  wrapBadges: true,
                  runnerName: widget.runnerName,
                  diffReviewed:
                      diffReviewedDigest ==
                      (accepted.review['revision'] as Map)['digest'],
                  onDiff: busy || previewOwner is! AutomationDiffService
                      ? null
                      : reviewDiff,
                ),
                ExpansionTile(
                  title: Text(l.automationServiceKeys),
                  children: [
                    fact(
                      l.automationServiceIdentity,
                      accepted.service['identityId'] as String,
                    ),
                    SelectableText(
                      accepted.service['signingFingerprint'] as String,
                    ),
                    SelectableText(
                      accepted.service['recipientFingerprint'] as String,
                    ),
                  ],
                ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('service-close'),
          onPressed: () => Navigator.of(context).pop(),
          child: Text(l.commonClose),
        ),
        if (accepted != null && !attempted)
          TextButton(
            key: const ValueKey('service-edit'),
            onPressed: busy ? null : edit,
            child: Text(l.commonBack),
          ),
        if (accepted == null &&
            !attempted &&
            !uncertain &&
            !existing &&
            widget.intent != null)
          FilledButton(
            key: const ValueKey('service-prepare'),
            onPressed: busy || revokeReviewed ? null : prepare,
            child: Text(l.automationServicePreview),
          ),
        if (accepted != null && !attempted)
          FilledButton(
            key: const ValueKey('service-confirm'),
            onPressed:
                busy ||
                    revokeReviewed ||
                    accepted.review['git_source'] is Map &&
                        diffReviewedDigest !=
                            (accepted.review['revision'] as Map)['digest']
                ? null
                : confirm,
            child: Text(l.automationServiceConfirm),
          ),
      ],
    );
  }
}
