import 'dart:async';

import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_runner_candidate.dart';
import 'package:consolecrypt/automation/automation_runner_cleanup.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String runnerCleanupReasonText(AppLocalizations l, RunnerCleanupReason reason) => switch (reason) {
  RunnerCleanupReason.unsupported || RunnerCleanupReason.invalidResponse => l.automationCleanupUnsupported,
  RunnerCleanupReason.nonRoot => l.automationCleanupNonRoot,
  RunnerCleanupReason.currentInterpreter => l.automationCleanupCurrentInterpreter,
  RunnerCleanupReason.cancelled => l.automationStateCancelled,
  RunnerCleanupReason.selected => l.automationCleanupSelected,
  RunnerCleanupReason.registered => l.automationCleanupRegistered,
  RunnerCleanupReason.busy => l.automationCleanupBusy,
  RunnerCleanupReason.catalogUnavailable => l.automationCleanupCatalogUnavailable,
  RunnerCleanupReason.expired => l.automationCleanupExpired,
  RunnerCleanupReason.budget || RunnerCleanupReason.timeout => l.automationCleanupBudget,
  RunnerCleanupReason.identityChanged ||
  RunnerCleanupReason.candidateChanged ||
  RunnerCleanupReason.receiptChanged ||
  RunnerCleanupReason.treeChanged ||
  RunnerCleanupReason.quarantineChanged ||
  RunnerCleanupReason.contextChanged => l.automationCleanupChanged,
  RunnerCleanupReason.quarantineFailed || RunnerCleanupReason.failed => l.automationCleanupFailed,
  RunnerCleanupReason.bootstrapFailed => l.automationCleanupBootstrapFailed,
  RunnerCleanupReason.commandTooLarge => l.automationCleanupCommandTooLarge,
  _ => l.automationCleanupUnsafe,
};

/// Scan and plan are read-only. Only an explicit confirmation consumes a Native
/// one-use token; no UI path or eligibility flag is accepted by commit.
class AutomationRunnerCleanupDialog extends ConsumerStatefulWidget {
  const AutomationRunnerCleanupDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.target,
    required this.contextCurrent,
    this.selectedPython,
    this.contextChanges,
    this.targetLabel,
    this.onMutationAttempted,
    this.expectedCandidate,
  });
  final AutomationScope scope;
  final AutomationRunnerCleanupService service;
  final AutomationJson target;
  final String? selectedPython, targetLabel;
  final bool Function() contextCurrent;
  final Listenable? contextChanges;
  final VoidCallback? onMutationAttempted;
  final RunnerCandidateSelection? expectedCandidate;
  @override
  ConsumerState<AutomationRunnerCleanupDialog> createState() => _AutomationRunnerCleanupDialogState();
}

class _AutomationRunnerCleanupDialogState extends ConsumerState<AutomationRunnerCleanupDialog> {
  RunnerCleanupScan? scan;
  RunnerCleanupCandidate? focusedCandidate;
  RunnerCleanupPlan? plan, committing;
  RunnerCleanupResult? result;
  DateTime? deadline;
  Object? error;
  String? operationId;
  int generation = 0;
  bool busy = false, interrupted = false;
  final scroll = ScrollController(keepScrollOffset: false);
  late final expectedContext = runnerCleanupContext(widget.target, widget.selectedPython);
  late final Timer timer;
  Timer? expiryTimer;
  bool get scopeCurrent => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get contextCurrent =>
      widget.contextCurrent() && runnerCleanupContext(widget.target, widget.selectedPython) == expectedContext;
  bool current(int token) => scopeCurrent && contextCurrent && token == generation;
  AppLocalizations get l => context.l10n;

  @override
  void initState() {
    super.initState();
    widget.contextChanges?.addListener(checkContext);
    timer = Timer.periodic(const Duration(seconds: 1), (_) => checkContext());
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (scopeCurrent && contextCurrent) refresh();
    });
  }

  @override
  void dispose() {
    generation++;
    timer.cancel();
    expiryTimer?.cancel();
    widget.contextChanges?.removeListener(checkContext);
    cancelNative();
    scroll.dispose();
    super.dispose();
  }

  void showStart(int token) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (current(token) && scroll.hasClients) scroll.jumpTo(0);
    });
  }

  void cancelNative() {
    final id = operationId;
    operationId = null;
    if (id != null) {
      unawaited(widget.service.cancelRunnerCleanup(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    }
  }

  void checkContext() {
    if (!scopeCurrent) return;
    final expired = plan != null && deadline != null && !DateTime.now().isBefore(deadline!);
    if (!contextCurrent || expired) {
      if (error is AutomationRunnerCleanupFailed &&
          (error as AutomationRunnerCleanupFailed).reason == RunnerCleanupReason.contextChanged &&
          !contextCurrent) {
        return;
      }
      generation++;
      expiryTimer?.cancel();
      cancelNative();
      setState(() {
        interrupted = committing != null || interrupted;
        scan = null;
        plan = null;
        committing = null;
        deadline = null;
        busy = false;
        error = AutomationRunnerCleanupFailed(
          expired ? RunnerCleanupReason.expired : RunnerCleanupReason.contextChanged,
        );
      });
    }
  }

  void cancelOperation() {
    generation++;
    expiryTimer?.cancel();
    cancelNative();
    setState(() {
      interrupted = committing != null;
      scan = null;
      plan = null;
      committing = null;
      busy = false;
      error = null;
      deadline = null;
    });
  }

  Future<void> refresh({bool keepResult = false}) async {
    if (busy || !scopeCurrent || !contextCurrent) {
      checkContext();
      return;
    }
    final token = ++generation;
    expiryTimer?.cancel();
    setState(() {
      busy = true;
      error = null;
      scan = null;
      plan = null;
      deadline = null;
      if (!keepResult) {
        result = null;
        interrupted = false;
      }
      operationId = ObjectId.generate().value;
    });
    final id = operationId!;
    RunnerCleanupCandidate? autoReview;
    try {
      final value = await widget.service.scanRunnerCleanup(
        widget.target,
        operationId: id,
        selectedPython: widget.selectedPython,
      );
      if (current(token)) {
        setState(() {
          scan = value;
          focusedCandidate = null;
        });
        final expected = widget.expectedCandidate;
        if (expected != null) {
          final exact = expected.resolve(value.candidates);
          setState(() => focusedCandidate = exact);
          if (exact.eligible) autoReview = exact;
        }
      }
    } catch (e) {
      if (current(token)) setState(() => error = e);
    } finally {
      if (scopeCurrent && generation == token) {
        setState(() {
          busy = false;
          operationId = null;
        });
      }
      if (current(token)) showStart(token);
      checkContext();
    }
    final ready = autoReview;
    if (ready != null && current(token)) await review(ready);
  }

  Future<void> review(RunnerCleanupCandidate candidate) async {
    if (busy || !candidate.eligible || !scopeCurrent || !contextCurrent) {
      checkContext();
      return;
    }
    final token = ++generation;
    setState(() {
      busy = true;
      result = null;
      error = null;
      interrupted = false;
      operationId = ObjectId.generate().value;
    });
    final id = operationId!;
    try {
      final value = await widget.service.planRunnerCleanup(
        widget.target,
        [candidate.id],
        operationId: id,
        selectedPython: widget.selectedPython,
      );
      if (value.candidates.length != 1 ||
          value.candidates.single.id != candidate.id ||
          runnerCleanupContext(value.target, widget.selectedPython) != expectedContext) {
        throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.invalidResponse);
      }
      if (!runnerCleanupSameCandidate(candidate, value.candidates.single) ||
          (widget.expectedCandidate != null && !widget.expectedCandidate!.matches(value.candidates.single))) {
        throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.candidateChanged);
      }
      if (current(token)) {
        setState(() {
          plan = value;
          deadline = DateTime.now().add(Duration(seconds: value.expiresInSeconds));
        });
        expiryTimer?.cancel();
        expiryTimer = Timer(Duration(seconds: value.expiresInSeconds), () {
          if (scopeCurrent && identical(plan, value)) {
            generation++;
            cancelNative();
            setState(() {
              plan = null;
              deadline = null;
              scan = null;
              busy = false;
              error = const AutomationRunnerCleanupFailed(RunnerCleanupReason.expired);
            });
          }
        });
      }
    } catch (e) {
      if (current(token)) setState(() => error = e);
    } finally {
      if (scopeCurrent && generation == token) {
        setState(() {
          busy = false;
          operationId = null;
        });
      }
      if (current(token)) showStart(token);
      checkContext();
    }
  }

  Future<void> commit() async {
    final approved = plan;
    if (busy ||
        approved == null ||
        !scopeCurrent ||
        !contextCurrent ||
        deadline == null ||
        !DateTime.now().isBefore(deadline!)) {
      checkContext();
      return;
    }
    final token = ++generation;
    expiryTimer?.cancel();
    // Remove the token from actionable state before the first asynchronous call.
    setState(() {
      plan = null;
      deadline = null;
      committing = approved;
      scan = null;
      result = null;
      error = null;
      busy = true;
      operationId = ObjectId.generate().value;
    });
    final id = operationId!;
    widget.onMutationAttempted?.call();
    var refreshList = false;
    try {
      final value = await widget.service.commitRunnerCleanup(
        approved.token,
        operationId: id,
        selectedPython: widget.selectedPython,
      );
      final ids = approved.candidates.map((c) => c.id).toSet();
      if (value.removed.length != ids.length || !value.removed.every((r) => ids.contains(r.id))) {
        throw const AutomationRunnerCleanupFailed(RunnerCleanupReason.invalidResponse);
      }
      if (current(token)) {
        setState(() => result = value);
        refreshList = true;
      }
    } catch (e) {
      if (current(token)) {
        setState(() {
          error = e;
          interrupted = true;
        });
      }
    } finally {
      if (scopeCurrent && generation == token) {
        setState(() {
          busy = false;
          committing = null;
          operationId = null;
        });
      }
      if (current(token)) showStart(token);
      checkContext();
    }
    if (refreshList && current(token)) {
      if (widget.expectedCandidate != null) {
        if (!mounted) return;
        Navigator.pop(context);
      } else {
        await refresh(keepResult: true);
      }
    }
  }

  Widget counts(int? nodes, int? bytes) => Wrap(
    spacing: 16,
    runSpacing: 4,
    children: [
      Text('${l.automationCleanupNodes}: ${nodes?.toString() ?? l.automationCleanupUnknownCount}'),
      Text('${l.automationCleanupBytes}: ${bytes?.toString() ?? l.automationCleanupUnknownCount}'),
    ],
  );
  Widget candidateView(RunnerCleanupCandidate candidate, {bool reviewing = false}) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 6),
    child: ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Expanded(child: SelectableText(candidate.path, style: GlassTokens.of(context).typography.mono)),
              if (!reviewing && candidate.eligible)
                IconButton(
                  key: ValueKey('automation-cleanup-review-${candidate.id}'),
                  tooltip: l.automationCleanupReview,
                  onPressed: busy || !contextCurrent ? null : () => review(candidate),
                  icon: Icon(Icons.delete_outline, color: Theme.of(context).colorScheme.error),
                ),
            ],
          ),
          if (candidate.quarantined)
            Align(
              alignment: Alignment.centerLeft,
              child: GlassBadge(label: l.automationCleanupQuarantine, tone: GlassTone.warning),
            ),
          counts(candidate.nodes, candidate.bytes),
          if (!candidate.eligible)
            Text(runnerCleanupReasonText(l, candidate.blockedReason ?? RunnerCleanupReason.unsafeEntry)),
          if (reviewing && candidate.paths != null) ...[
            const SizedBox(height: 8),
            Text(l.automationPythonPath),
            SelectableText(candidate.paths!['python']!),
            Text(l.automationModulePath),
            SelectableText(candidate.paths!['module_dir']!),
            Text(l.automationStatePath),
            SelectableText(candidate.paths!['root']!),
          ],
          ExpansionTile(
            key: ValueKey('automation-cleanup-receipt-${candidate.id}'),
            title: Text(l.automationCleanupTechnical),
            children: [
              if (candidate.environmentId != null) SelectableText(candidate.environmentId!),
              if (candidate.revision != null) SelectableText(candidate.revision!),
              if (candidate.receiptSha256 != null) SelectableText(candidate.receiptSha256!),
            ],
          ),
        ],
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final displayed = plan ?? committing;
    final identityUser = scan?.identity?['user'], identityHome = scan?.identity?['home'];
    return AutomationScopeGuard(
      scope: widget.scope,
      builder: (_) => AlertDialog(
        title: Text(displayed == null ? l.automationCleanupTitle : l.automationCleanupConfirmTitle),
        content: SizedBox(
          width: 760,
          height: MediaQuery.sizeOf(context).width < 700
              ? (MediaQuery.sizeOf(context).height * .72).clamp(320, 680)
              : 520,
          child: ListView(
            key: const ValueKey('automation-cleanup-scroll'),
            controller: scroll,
            children: [
              if (widget.targetLabel != null) Text(widget.targetLabel!, style: Theme.of(context).textTheme.titleSmall),
              if (displayed == null) ...[Text(l.automationCleanupScope), const SizedBox(height: 12)],
              Text(l.automationCleanupCurrentProfile),
              const SizedBox(height: 12),
              if (busy) const LinearProgressIndicator(),
              if (error != null)
                InfoBanner(
                  message: error is RunnerCandidateUnavailable
                      ? l.automationRunnerCandidateUnavailable
                      : error is AutomationRunnerCleanupFailed
                      ? runnerCleanupReasonText(l, (error as AutomationRunnerCleanupFailed).reason)
                      : errorMessage(l, error!),
                  key: const ValueKey('automation-cleanup-error'),
                  tone: BannerTone.danger,
                ),
              if (widget.expectedCandidate?.python == widget.selectedPython)
                Text(l.automationRunnerSelectionProtectedHint),
              if (interrupted) Text(l.automationCleanupInterrupted),
              if (result case final done?) ...[
                Text(l.automationCleanupSuccess, key: const ValueKey('automation-cleanup-success')),
                for (final row in done.removed) counts(row.nodes, row.bytes),
              ],
              if (displayed != null) ...[
                Text(l.automationCleanupStateNotice),
                const SizedBox(height: 12),
                Text(l.automationCleanupExpiry),
                const SizedBox(height: 8),
                Text(l.automationCleanupInterrupted),
                for (final candidate in displayed.candidates) candidateView(candidate, reviewing: true),
              ] else if (scan case final value?) ...[
                if (identityUser != null || identityHome != null)
                  ExpansionTile(
                    title: Text(l.automationCleanupIdentity),
                    children: [
                      if (identityUser != null) SelectableText('$identityUser'),
                      if (identityHome != null) SelectableText('$identityHome'),
                    ],
                  ),
                if (value.candidates.isEmpty) Text(l.automationCleanupEmpty),
                if (widget.expectedCandidate == null)
                  for (final candidate in value.candidates) candidateView(candidate)
                else if (focusedCandidate case final exact?)
                  candidateView(exact)
                else
                  // Blocked rows may intentionally omit paths; show their safe
                  // Native reasons, never guess an ID or offer another target.
                  for (final candidate in value.candidates.where((candidate) => !candidate.eligible))
                    candidateView(candidate),
              ],
            ],
          ),
        ),
        actions: [
          if (plan != null && !busy) ...[
            TextButton(
              key: const ValueKey('automation-cleanup-back'),
              onPressed: () => setState(() {
                plan = null;
                deadline = null;
              }),
              child: Text(l.commonBack),
            ),
            GlassButton.destructive(
              key: const ValueKey('automation-cleanup-confirm'),
              label: l.automationCleanupConfirm,
              onPressed: contextCurrent ? commit : null,
            ),
          ] else if (!busy)
            GlassButton(
              key: const ValueKey('automation-cleanup-scan'),
              label: l.automationCleanupScan,
              onPressed: contextCurrent ? refresh : null,
            ),
          if (busy)
            TextButton(
              key: const ValueKey('automation-cleanup-cancel'),
              onPressed: cancelOperation,
              child: Text(l.commonCancel),
            ),
          TextButton(
            key: const ValueKey('automation-cleanup-close'),
            onPressed: () => Navigator.pop(context),
            child: Text(l.commonClose),
          ),
        ],
      ),
    );
  }
}
