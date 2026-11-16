import 'dart:async';

import 'package:consolecrypt/automation/automation_dependency_controller.dart';
import 'package:consolecrypt/automation/automation_dependency_models.dart';
import 'package:consolecrypt/automation/automation_diff_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Root integrates these strings into the existing RU/EN localization catalog;
/// no human UI label is taken from a Helper, archive or model proposal.
final class DependencyInstallerLabels {
  const DependencyInstallerLabels({
    required this.title,
    required this.kind,
    required this.role,
    required this.collection,
    required this.name,
    required this.version,
    required this.url,
    required this.sha256,
    required this.runner,
    required this.review,
    required this.download,
    required this.diff,
    required this.install,
    required this.cancel,
    required this.close,
    required this.firstInstall,
    required this.managedOnly,
    required this.profileUnsupported,
    required this.excludedEntries,
    required this.platformUnsupported,
    required this.networkNotice,
    required this.preparing,
    required this.published,
    required this.uncertain,
    required this.cancelRequested,
    required this.failureText,
  });
  final String title,
      kind,
      role,
      collection,
      name,
      version,
      url,
      sha256,
      runner,
      review,
      download,
      diff,
      install,
      cancel,
      close,
      firstInstall,
      managedOnly,
      profileUnsupported,
      excludedEntries,
      platformUnsupported,
      networkNotice,
      preparing,
      published,
      uncertain,
      cancelRequested;
  final String Function(DependencyFailureCode) failureText;
}

class AutomationDependencyDialog extends ConsumerStatefulWidget {
  const AutomationDependencyDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.project,
    required this.labels,
    required this.sourceCurrent,
    this.originalLive,
  });
  final AutomationScope scope;
  final AutomationDependencyService service;
  final AutomationProject project;
  final DependencyInstallerLabels labels;
  // Existing workspace's captured project/revision/request/service predicate.
  // UI freshness only; no serialized permission or Native bypass.
  final bool Function() sourceCurrent;
  final ValueListenable<bool>? originalLive;
  @override
  ConsumerState<AutomationDependencyDialog> createState() =>
      _AutomationDependencyDialogState();
}

class _AutomationDependencyDialogState
    extends ConsumerState<AutomationDependencyDialog> {
  final name = TextEditingController(),
      version = TextEditingController(),
      url = TextEditingController(),
      sha = TextEditingController();
  String kind = 'role';
  String? selectedRunner;
  DependencyInstallerController? controller;
  Timer? ticker;
  bool get current =>
      mounted &&
      identical(ref.read(automationScopeProvider), widget.scope) &&
      (widget.originalLive?.value ?? true) &&
      widget.sourceCurrent();
  bool get managed =>
      widget.project.sourceKind == 'managed' &&
      !widget.project.readOnly &&
      widget.project.revision != null;
  @override
  void initState() {
    super.initState();
    ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      final c = controller;
      if (c != null) unawaited(c.poll());
    });
  }

  void changed() {
    controller?.changed();
  }

  void _notify() {
    if (mounted) setState(() {});
  }

  void _clear() {
    controller?.removeListener(_notify);
    controller?.dispose();
    controller = null;
    selectedRunner = null;
    for (final text in [name, version, url, sha]) {
      text.clear();
    }
    kind = 'role';
  }

  @override
  void didUpdateWidget(covariant AutomationDependencyDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.scope, widget.scope) ||
        !identical(oldWidget.service, widget.service) ||
        oldWidget.project.id != widget.project.id ||
        oldWidget.project.revision != widget.project.revision) {
      _clear();
    }
  }

  void choose(AutomationRunnerProfile runner) {
    if (!current || controller?.busy == true) return;
    controller?.removeListener(_notify);
    controller?.dispose();
    late final DependencyInstallerController next;
    next = DependencyInstallerController(
      client: DependencyActionClient(widget.service),
      projectId: widget.project.id,
      projectRevision: widget.project.revision!,
      runnerId: runner.id,
      runnerRevision: runner.revision!,
      managed: managed,
      isCurrent: () {
        if (!current || !identical(controller, next)) return false;
        final rows = ref.read(automationRunnersProvider).asData?.value?.runners;
        return rows?.any(
              (row) =>
                  row.id == runner.id &&
                  row.revision == runner.revision &&
                  !row.readOnly,
            ) ==
            true;
      },
    );
    next.addListener(_notify);
    setState(() {
      selectedRunner = runner.id;
      controller = next;
    });
  }

  Future<void> review() async {
    if (!current) return;
    await controller?.reviewSource(
      DependencyProposal(
        kind: kind,
        name: name.text,
        version: version.text,
        url: url.text,
        archiveSha256: sha.text,
      ),
    );
  }

  Future<void> diff() async {
    final c = controller, verified = controller?.evidence;
    if (!current || c == null || verified == null) return;
    final result = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (_) => DependencyOriginalGuard(
        original: widget.originalLive,
        builder: (_) => AutomationDiffDialog(
          scope: widget.scope,
          service: c,
          projectId: widget.project.id,
          digest: verified.candidateDigest,
        ),
      ),
    );
    if (current && identical(controller, c) && result == true) {
      c.markDiffViewed();
    }
  }

  Future<void> close() async {
    final c = controller;
    if (c?.phase == DependencyInstallerPhase.installing ||
        c?.phase == DependencyInstallerPhase.cancelling) {
      await c!.cancel();
      return;
    }
    await c?.cancel();
    if (mounted) Navigator.pop(context, c?.published);
  }

  @override
  void dispose() {
    ticker?.cancel();
    controller?.removeListener(_notify);
    controller?.dispose();
    for (final text in [name, version, url, sha]) {
      text.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(automationScopeProvider, (_, next) {
      if (!identical(next, widget.scope)) {
        _clear();
      }
    });
    if (!identical(ref.watch(automationScopeProvider), widget.scope)) {
      return const SizedBox.shrink();
    }
    final rows =
        ref.watch(automationRunnersProvider).asData?.value?.runners ??
        const <AutomationRunnerProfile>[];
    final c = controller, l = widget.labels, phase = c?.phase;
    final editing =
        current &&
        managed &&
        (c == null ||
            {
              DependencyInstallerPhase.editing,
              DependencyInstallerPhase.sourceReviewed,
              DependencyInstallerPhase.failed,
              DependencyInstallerPhase.cancelled,
            }.contains(phase));
    final cannotPop =
        phase == DependencyInstallerPhase.installing ||
        phase == DependencyInstallerPhase.cancelling;
    final selected = c?.review;
    return PopScope(
      canPop: !cannotPop,
      child: AlertDialog(
        title: Text(l.title),
        content: SizedBox(
          width: 680,
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(l.firstInstall),
                if (!managed) Text(l.managedOnly),
                DropdownButtonFormField<String>(
                  key: const ValueKey('dependency-runner'),
                  initialValue:
                      rows.any(
                        (r) =>
                            r.id == selectedRunner &&
                            !r.readOnly &&
                            r.revision != null,
                      )
                      ? selectedRunner
                      : null,
                  decoration: InputDecoration(labelText: l.runner),
                  items: [
                    for (final row in rows)
                      if (!row.readOnly && row.revision != null)
                        DropdownMenuItem(
                          value: row.id,
                          child: Text(
                            '${row.name ?? row.id} \u00b7 ${row.revision}',
                          ),
                        ),
                  ],
                  onChanged: editing
                      ? (id) {
                          for (final row in rows) {
                            if (row.id == id) {
                              choose(row);
                              break;
                            }
                          }
                        }
                      : null,
                ),
                DropdownButtonFormField<String>(
                  key: const ValueKey('dependency-kind'),
                  initialValue: kind,
                  decoration: InputDecoration(labelText: l.kind),
                  items: [
                    DropdownMenuItem(value: 'role', child: Text(l.role)),
                    DropdownMenuItem(
                      value: 'collection',
                      child: Text(l.collection),
                    ),
                  ],
                  onChanged: editing
                      ? (value) {
                          if (value != null) {
                            setState(() {
                              kind = value;
                              changed();
                            });
                          }
                        }
                      : null,
                ),
                for (final field in [
                  (name, l.name, 'dependency-name'),
                  (version, l.version, 'dependency-version'),
                  (url, l.url, 'dependency-url'),
                  (sha, l.sha256, 'dependency-sha'),
                ])
                  TextField(
                    key: ValueKey(field.$3),
                    controller: field.$1,
                    enabled: editing,
                    autocorrect: false,
                    enableSuggestions: false,
                    decoration: InputDecoration(labelText: field.$2),
                    onChanged: (_) => changed(),
                    maxLength: field.$1 == url
                        ? 2048
                        : field.$1 == sha
                        ? 64
                        : 120,
                  ),
                if (selected != null) ...[
                  const SizedBox(height: 12),
                  Text(l.networkNotice),
                  SelectableText(selected.proposal.url),
                  SelectableText(selected.networkOrigin),
                  SelectableText(selected.proposal.archiveSha256),
                  SelectableText(selected.destination),
                  if (!selected.publicationSupported)
                    Text(switch (selected.publicationReason) {
                      'excluded_project_entries' => l.excludedEntries,
                      'publication_platform_unsupported' =>
                        l.platformUnsupported,
                      'managed_project_required' => l.managedOnly,
                      _ => l.profileUnsupported,
                    }),
                ],
                if (c?.busy == true) ...[
                  const LinearProgressIndicator(),
                  Text(
                    phase == DependencyInstallerPhase.cancelling
                        ? l.cancelRequested
                        : l.preparing,
                  ),
                ],
                if (c?.evidence case final evidence?) ...[
                  SelectableText(evidence.candidateDigest),
                  SelectableText(evidence.lockSha256),
                  SelectableText(evidence.receiptSha256),
                  TextButton(
                    key: const ValueKey('dependency-diff'),
                    onPressed: phase == DependencyInstallerPhase.candidateReview
                        ? diff
                        : null,
                    child: Text(l.diff),
                  ),
                ],
                if (c?.failure case final failure?)
                  InfoBanner(
                    key: const ValueKey('dependency-error'),
                    message: failure == DependencyFailureCode.recoveryRequired
                        ? l.uncertain
                        : l.failureText(failure),
                    tone: BannerTone.danger,
                  ),
                if (phase == DependencyInstallerPhase.published)
                  Text(l.published),
              ],
            ),
          ),
        ),
        actions: [
          if (editing)
            TextButton(
              key: const ValueKey('dependency-review'),
              onPressed: c == null ? null : review,
              child: Text(l.review),
            ),
          if (phase == DependencyInstallerPhase.sourceReviewed)
            FilledButton(
              key: const ValueKey('dependency-download'),
              onPressed: current && selected?.publicationSupported == true
                  ? () => c!.download()
                  : null,
              child: Text(l.download),
            ),
          if (phase == DependencyInstallerPhase.candidateReview)
            FilledButton(
              key: const ValueKey('dependency-install'),
              onPressed: c?.canInstall == true ? () => c!.install() : null,
              child: Text(l.install),
            ),
          TextButton(
            key: const ValueKey('dependency-cancel'),
            onPressed: close,
            child: Text(
              phase == DependencyInstallerPhase.published ||
                      phase == DependencyInstallerPhase.recoveryRequired
                  ? l.close
                  : l.cancel,
            ),
          ),
        ],
      ),
    );
  }
}

/// An existing operation's UI lifetime only. It removes THIS route, even when
/// an unrelated modal is above it. It never pops the global top route or grants
/// any Native action. A retired source notifier is never set true again.
class DependencyOriginalGuard extends StatelessWidget {
  const DependencyOriginalGuard({
    super.key,
    required this.original,
    required this.builder,
  });
  final ValueListenable<bool>? original;
  final WidgetBuilder builder;
  @override
  Widget build(BuildContext context) {
    final signal = original;
    if (signal == null) return builder(context);
    return ValueListenableBuilder<bool>(
      valueListenable: signal,
      builder: (context, live, _) {
        if (!live) {
          final route = ModalRoute.of(context);
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (context.mounted && route != null && route.isActive) {
              Navigator.of(context).removeRoute(route);
            }
          });
          return const SizedBox.shrink();
        }
        return builder(context);
      },
    );
  }
}
