import 'dart:async';

import 'package:consolecrypt/automation/automation_runner_candidate.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_windows_prerequisites.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

typedef WslPreparedSelection = ({
  String distribution,
  String? user,
  AutomationJson paths,
});

/// Read-only diagnostics and explicit handoff; never installs or executes commands.
class AutomationWslPrerequisiteWizard extends StatefulWidget {
  const AutomationWslPrerequisiteWizard({
    super.key,
    required this.original,
    required this.discover,
    required this.openTerminal,
    this.windows,
    this.setup,
    this.nativeScope,
    this.initialProfiles,
    this.initialDistribution,
    this.initialUser,
    this.onPrepared,
  });
  final bool Function() original;
  final Future<LocalShellProfiles> Function() discover;
  final Future<void> Function(LocalShellProfile) openTerminal;
  final AutomationWindowsPrerequisiteService? windows;
  final AutomationRunnerSetupService? setup;
  final RuntimeNoticeScope? nativeScope;
  final LocalShellProfiles? initialProfiles;
  final String? initialDistribution, initialUser;
  final ValueChanged<WslPreparedSelection>? onPrepared;
  @override
  State<AutomationWslPrerequisiteWizard> createState() =>
      _AutomationWslPrerequisiteWizardState();
}

class _AutomationWslPrerequisiteWizardState
    extends State<AutomationWslPrerequisiteWizard> {
  late final user = TextEditingController(text: widget.initialUser ?? '');
  LocalShellProfiles? profiles;
  LocalShellProfile? distribution, shell;
  WindowsPrerequisiteSnapshot? windows;
  AutomationJson? linux;
  String? linuxIdentity, operation;
  int generation = 0;
  bool busy = false, failed = false, showPlan = false;
  int? candidate;
  bool get current => mounted && widget.original();
  String get identity => '${distribution?.id}\u0000${user.text.trim()}';
  @override
  void initState() {
    super.initState();
    profiles = widget.initialProfiles;
    distribution = profiles?.profiles
        .where(
          (p) =>
              p.kind == 'wsl' && p.distribution == widget.initialDistribution,
        )
        .firstOrNull;
  }

  void cancel() {
    final id = operation;
    if (id != null && widget.setup != null) {
      unawaited(
        widget.setup!
            .cancelRunnerSetup(id)
            .then<void>((_) {}, onError: (Object _, StackTrace _) {}),
      );
    }
    operation = null;
  }

  @override
  void dispose() {
    generation++;
    cancel();
    user.dispose();
    super.dispose();
  }

  bool same(int token) => current && token == generation;
  Future<void> inspectWindows() async {
    final service = widget.windows, scope = widget.nativeScope;
    if (!current || busy || service == null || scope == null || widget.setup == null) return;
    final token = ++generation, id = ObjectId.generate().value;
    operation = id;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final raw = await service.inspectWindowsPrerequisites(
        operationId: id,
        profileId: scope.profileId,
        notificationScopeId: scope.nonce,
      );
      if (same(token)) {
        setState(() => windows = WindowsPrerequisiteSnapshot.parse(raw, scope));
      }
    } catch (_) {
      if (same(token)) {
        setState(() {
          windows = null;
          failed = true;
        });
      }
    } finally {
      if (same(token)) setState(() => busy = false);
      if (operation == id) operation = null;
    }
  }

  Future<void> refreshProfiles() async {
    if (!current || busy) return;
    final token = ++generation;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final result = await widget.discover();
      if (same(token)) {
        setState(() {
          final oldDistribution = distribution, oldShell = shell;
          profiles = result;
          distribution = result.profiles
              .where(
                (p) =>
                    p.kind == 'wsl' &&
                    p.id == oldDistribution?.id &&
                    p.distribution == oldDistribution?.distribution,
              )
              .firstOrNull;
          shell = result.profiles
              .where(
                (p) =>
                    p.id == oldShell?.id &&
                    p.kind == oldShell?.kind &&
                    p.distribution == oldShell?.distribution,
              )
              .firstOrNull;
          linux = null;
          linuxIdentity = null;
          candidate = null;
        });
      }
    } catch (_) {
      if (same(token)) setState(() => failed = true);
    } finally {
      if (same(token)) setState(() => busy = false);
    }
  }

  Future<void> inspectLinux() async {
    final selected = distribution, setup = widget.setup;
    if (!current || busy || selected?.distribution == null || setup == null) {
      return;
    }
    final name = user.text.trim();
    if ((name.isNotEmpty &&
            !RegExp(r'^[a-zA-Z0-9_.-]{1,64}$').hasMatch(name)) ||
        name.startsWith('-')) {
      setState(() => failed = true);
      return;
    }
    final expected = identity,
        token = ++generation,
        id = ObjectId.generate().value;
    operation = id;
    setState(() {
      busy = true;
      failed = false;
      linux = null;
      candidate = null;
    });
    try {
      final result = await setup.setupRunner(
        {
          'kind': 'wsl',
          'distribution': selected!.distribution,
          'user': name.isEmpty ? null : name,
        },
        operationId: id,
        install: false,
      );
      if (same(token) && identity == expected) {
        if (result['version'] != 1 ||
            result['users'] is! List ||
            result['missing'] is! List ||
            result['candidates'] is! List ||
            (result['candidates'] as List).length > 32 ||
            (result['missing'] as List).length > 32) {
          throw const FormatException();
        }
        for (final row in result['candidates'] as List) {
          RunnerCandidateSelection.fromPaths(
            Map<String, dynamic>.from(row as Map),
          );
        }
        setState(() {
          linux = result;
          linuxIdentity = expected;
        });
      }
    } catch (_) {
      if (same(token)) setState(() => failed = true);
    } finally {
      if (same(token)) setState(() => busy = false);
      if (operation == id) operation = null;
    }
  }

  Future<void> handoffTerminal() async {
    final selected = shell;
    if (!current ||
        busy ||
        selected == null ||
        !showPlan ||
        profiles?.profiles.any((p) => identical(p, selected)) != true) {
      return;
    }
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      await widget.openTerminal(selected);
    } catch (_) {
      if (current) setState(() => failed = true);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  void usePrepared() {
    final data = linux, index = candidate, selected = distribution;
    if (!current ||
        busy ||
        data == null ||
        linuxIdentity != identity ||
        index == null ||
        selected?.distribution == null) {
      return;
    }
    final rows = data['candidates'] as List;
    if (index >= rows.length) return;
    final paths = Map<String, dynamic>.from(rows[index] as Map);
    try {
      RunnerCandidateSelection.fromPaths(paths);
    } catch (_) {
      setState(() => failed = true);
      return;
    }
    final outdated = (data['outdated_candidates'] as List? ?? const []);
    if (outdated.contains(paths['python'])) {
      setState(() => failed = true);
      return;
    }
    final actualUser = user.text.trim().isNotEmpty
        ? user.text.trim()
        : data['user'];
    if (actualUser is! String ||
        !RegExp(r'^[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,63}$').hasMatch(actualUser) ||
        actualUser == 'root' ||
        data['non_root'] != true ||
        data['wsl2'] != true) {
      setState(() => failed = true);
      return;
    }
    widget.onPrepared?.call((
      distribution: selected!.distribution!,
      user: actualUser,
      paths: Map.unmodifiable(paths),
    ));
  }

  String? safeFact(Object? value) =>
      value is String &&
          value.isNotEmpty &&
          value.length <= 256 &&
          !RegExp(r'[\x00-\x1f\x7f]').hasMatch(value)
      ? value
      : null;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, host = windows, scan = linux;
    String feature(WindowsFeatureFact? value) => switch (value?.state) {
      WindowsFeatureObservation.enabled => l.automationWslWizardFeatureEnabled,
      WindowsFeatureObservation.disabled =>
        l.automationWslWizardFeatureDisabled,
      WindowsFeatureObservation.enablePending =>
        l.automationWslWizardFeatureEnablePending,
      WindowsFeatureObservation.disablePending =>
        l.automationWslWizardFeatureDisablePending,
      _ =>
        value?.reason == WindowsFeatureUnknown.requiresElevation
            ? l.automationWslWizardElevationRequired
            : l.commonUnknown,
    };
    Widget card(String title, List<Widget> children) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(title, style: Theme.of(context).textTheme.titleMedium),
          ...children,
        ],
      ),
    );
    final distros =
        profiles?.profiles
            .where((p) => p.kind == 'wsl' && p.distribution != null)
            .toList() ??
        const <LocalShellProfile>[];
    final terminals =
        profiles?.profiles
            .where(
              (p) =>
                  p.kind == 'powershell' || p.kind == 'cmd' || p.kind == 'wsl',
            )
            .toList() ??
        const <LocalShellProfile>[];
    final rows = scan?['candidates'] as List? ?? const [];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InfoBanner(message: l.automationWslWizardReadOnly),
        if (busy)
          const LinearProgressIndicator(key: ValueKey('wsl-wizard-busy')),
        if (failed)
          InfoBanner(
            key: const ValueKey('wsl-wizard-error'),
            tone: BannerTone.danger,
            message: l.automationWslWizardFailed,
          ),
        card(l.automationWslWizardWindows, [
          Text(l.automationWslWizardSubsystem(feature(host?.subsystem))),
          Text(
            l.automationWslWizardVirtualMachine(feature(host?.virtualMachine)),
          ),
          Text(switch (host?.elevation) {
            WindowsElevationObservation.elevated =>
              l.automationWslWizardElevated,
            WindowsElevationObservation.notElevated =>
              l.automationWslWizardNotElevated,
            _ => l.automationWslWizardElevationUnknown,
          }),
          Text(switch (host?.reboot) {
            WindowsRebootObservation.indicatorsPresent =>
              l.automationWslWizardRebootPresent,
            WindowsRebootObservation.notObserved =>
              l.automationWslWizardRebootNotObserved,
            _ => l.automationWslWizardRebootUnknown,
          }),
          if (host != null)
            Text(
              l.automationWslWizardCaptured(host.capturedAt.toIso8601String()),
            ),
          if (widget.windows == null || widget.nativeScope == null || widget.setup == null)
            Text(l.automationWslWizardUnavailable),
          OutlinedButton(
            key: const ValueKey('wsl-wizard-inspect-windows'),
            onPressed:
                current &&
                    !busy &&
                    widget.windows != null && widget.setup != null &&
                    widget.nativeScope != null
                ? inspectWindows
                : null,
            child: Text(l.automationWslWizardInspect),
          ),
        ]),
        card(l.automationWslWizardEnvironment, [
          Text(l.automationWslDiscoveryHint),
          OutlinedButton(
            key: const ValueKey('wsl-wizard-discover'),
            onPressed: current && !busy ? refreshProfiles : null,
            child: Text(l.automationWslRefresh),
          ),
          if (profiles?.wslDiscovery != WslDiscoveryStatus.available)
            Text(l.automationWslWizardDiscoveryUnknown),
          GlassSelect<String?>(
            key: const ValueKey('wsl-wizard-distribution'),
            value: distribution?.id,
            expand: true,
            items: [
              GlassSelectItem(value: null, label: l.automationDistribution),
              for (final p in distros)
                GlassSelectItem(value: p.id, label: p.label),
            ],
            onChanged: busy
                ? null
                : (id) => setState(() {
                    distribution = distros.where((p) => p.id == id).firstOrNull;
                    linux = null;
                    linuxIdentity = null;
                    candidate = null;
                  }),
          ),
          TextField(
            key: const ValueKey('wsl-wizard-user'),
            controller: user,
            enabled: !busy,
            decoration: InputDecoration(labelText: l.automationLinuxUser),
            onChanged: (_) => setState(() {
              linux = null;
              linuxIdentity = null;
              candidate = null;
            }),
          ),
          OutlinedButton(
            key: const ValueKey('wsl-wizard-inspect-linux'),
            onPressed:
                current && !busy && distribution != null && widget.setup != null
                ? inspectLinux
                : null,
            child: Text(l.automationWslWizardInspectLinux),
          ),
          if (scan != null) ...[
            Text(l.automationWslWizardLinuxReport),
            for (final key in [
              'user',
              'python',
              'system',
              'machine',
              'release',
            ])
              if (safeFact(scan[key]) case final String value)
                Text(
                  '$key: $value',
                ), // l10n-ignore: fixed diagnostic field identifiers
            for (final missing in (scan['missing'] as List).take(16))
              Text(
                l.automationWslWizardMissing(switch (missing) {
                  'supported_python' => 'Python',
                  'python_venv' => 'python3-venv / ensurepip',
                  'non_root_user' => l.automationLinuxUser,
                  'bwrap' => 'bubblewrap',
                  'ssh' => 'OpenSSH',
                  'git' => 'Git',
                  _ => l.commonUnknown,
                }),
              ),
            if (rows.isEmpty) Text(l.automationWslWizardNoPrepared),
            if (rows.isNotEmpty)
              GlassSelect<int?>(
                key: const ValueKey('wsl-wizard-candidate'),
                value: candidate,
                expand: true,
                items: [
                  GlassSelectItem(
                    value: null,
                    label: l.automationWslWizardSelectPrepared,
                  ),
                  for (var i = 0; i < rows.length; i++)
                    GlassSelectItem(
                      value: i,
                      label:
                          safeFact((rows[i] as Map)['python']) ??
                          l.commonUnknown,
                    ),
                ],
                onChanged: busy ? null : (i) => setState(() => candidate = i),
              ),
            if (widget.onPrepared != null)
              OutlinedButton(
                key: const ValueKey('wsl-wizard-use-prepared'),
                onPressed: current && !busy && candidate != null
                    ? usePrepared
                    : null,
                child: Text(l.automationWslWizardUsePrepared),
              ),
            Text(l.automationWslWizardPreparedNotice),
          ],
        ]),
        OutlinedButton(
          key: const ValueKey('wsl-wizard-review-plan'),
          onPressed: current && !busy
              ? () => setState(() => showPlan = true)
              : null,
          child: Text(l.automationWslWizardReviewPlan),
        ),
        if (showPlan)
          card(l.automationWslWizardPlan, [
            Text(
              host == null ||
                      host.subsystem.state ==
                          WindowsFeatureObservation.unknown ||
                      host.virtualMachine.state ==
                          WindowsFeatureObservation.unknown
                  ? l.automationWslWizardPlanUnknown
                  : l.automationWslWizardPlanKnown,
            ),
            Text(l.automationWslWizardPlanManual),
            if (host?.reboot == WindowsRebootObservation.indicatorsPresent)
              Text(l.automationWslWizardRebootPresent),
            Text(l.automationWslWizardTerminalNotice),
            GlassSelect<String?>(
              key: const ValueKey('wsl-wizard-terminal'),
              value: shell?.id,
              expand: true,
              items: [
                GlassSelectItem(
                  value: null,
                  label: l.automationWslWizardChooseTerminal,
                ),
                for (final p in terminals)
                  GlassSelectItem(value: p.id, label: p.label),
              ],
              onChanged: busy
                  ? null
                  : (id) => setState(
                      () => shell = terminals
                          .where((p) => p.id == id)
                          .firstOrNull,
                    ),
            ),
            OutlinedButton(
              key: const ValueKey('wsl-wizard-open-terminal'),
              onPressed: current && !busy && shell != null
                  ? handoffTerminal
                  : null,
              child: Text(l.automationWslWizardOpenTerminal),
            ),
            Text(l.automationWslWizardRecheck),
          ]),
      ],
    );
  }
}
