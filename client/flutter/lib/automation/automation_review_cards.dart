import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// Presentation of native frozen review facts; no authority is derived here.
class AutomationReviewCards extends StatelessWidget {
  const AutomationReviewCards({
    super.key,
    required this.review,
    required this.runnerName,
    required this.diffReviewed,
    required this.onDiff,
    this.wrapBadges = false,
  });
  final AutomationJson review;
  final String runnerName;
  final bool diffReviewed;
  final bool wrapBadges;
  final VoidCallback? onDiff;
  Widget fact(BuildContext context, String label, Object? value, {String? id}) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 6),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Expanded(flex: 2, child: Text(label, style: Theme.of(context).textTheme.labelMedium)),
        const SizedBox(width: 12),
        Expanded(flex: 3, child: SelectableText('$value', key: id == null ? null : ValueKey(id))),
      ],
    ),
  );
  Widget card(BuildContext context, String id, IconData icon, String title, List<Widget> children) => ContentSurface(
    key: ValueKey('automation-review-$id-card'),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Icon(icon, size: 18, color: Theme.of(context).colorScheme.primary),
            const SizedBox(width: 8),
            Expanded(child: Text(title, style: Theme.of(context).textTheme.titleSmall)),
          ],
        ),
        const SizedBox(height: 12),
        ...children,
      ],
    ),
  );
  Widget boundedBadge(BuildContext context, {required String id, required String label,
    GlassTone tone = GlassTone.neutral}) {
    if (!wrapBadges) return GlassBadge(key: ValueKey(id), label: label, tone: tone);
    final tokens = GlassTokens.of(context);
    return Text(label, key: ValueKey(id), style: tokens.typography.caption.copyWith(
      fontWeight: FontWeight.w600,
      color: tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(tone)));
  }
  String routeText(AutomationJson route) {
    final proxy = route['proxy'] as AutomationJson?, hops = (route['hops'] as List? ?? []).cast<AutomationJson>();
    return [
      if (proxy != null) '${proxy['name']} · ${proxy['kind']} · ${proxy['address']}:${proxy['port']}',
      hops.map((hop) => hop['endpoint']).join(' → '),
    ].where((s) => s.isNotEmpty).join('\n');
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, apply = review['mode'] == 'apply';
    final policy = review['policy'] as AutomationJson? ?? {}, origin = review['origin'] as AutomationJson?;
    final targets = (review['targets'] as List? ?? []).cast<AutomationJson>(),
        routes = (review['routes'] as List? ?? []).cast<AutomationJson>();
    final expiry = DateTime.fromMillisecondsSinceEpoch((review['expires_at'] as int) * 1000).toLocal();
    final cards = [
      card(context, 'playbook', Icons.description_outlined, l.automationReviewPlaybookTitle, [
        SelectableText(review['playbook'] as String, key: const ValueKey('automation-review-playbook')),
        fact(context, l.automationReviewProjectRevision, review['project_revision'] ?? l.automationResultMissing,
          id: 'automation-review-project-revision'),
        fact(context, l.automationRevision, (review['revision'] as AutomationJson?)?['digest'] ?? l.automationResultMissing,
          id: 'automation-review-project-digest'),
        const SizedBox(height: 12),
        Align(
          alignment: Alignment.centerLeft,
          child: boundedBadge(context,
            id: 'automation-review-mode-badge',
            label: apply ? l.automationApplyMode : l.automationCheckMode,
            tone: apply ? GlassTone.warning : GlassTone.info,
          ),
        ),
        const SizedBox(height: 8),
        Text(apply ? l.automationReviewApplyHint : l.automationCheckWarning),
        if (review['retry_origin'] != null) ...[const SizedBox(height: 8), Text(l.automationRetryWarning)],
        if (review['git_source'] is Map) ...[
          if ((review['git_source'] as Map)['snapshot_digest'] != (review['revision'] as Map?)?['digest'])
            Text(l.automationGitLocalChanges),
          const SizedBox(height: 8),
          TextButton.icon(
            key: const ValueKey('automation-review-diff'),
            onPressed: onDiff,
            icon: Icon(diffReviewed ? Icons.check : Icons.compare_arrows),
            label: Text(l.automationGitDiff),
          ),
        ],
      ]),
      card(context, 'runner', Icons.dns_outlined, l.automationReviewRunnerTitle, [
        SelectableText(runnerName, key: const ValueKey('automation-review-runner')),
        fact(context, l.automationReviewRunnerId, review['runner_id'], id: 'automation-review-runner-id'),
        if (origin != null) ...[
          const SizedBox(height: 8),
          fact(context, l.automationOrigin, switch (origin['kind']) {
            'wsl' => 'WSL · ${origin['distribution']} · ${origin['user']}',
            'remote' => l.automationRemoteOrigin,
            'service_remote' => l.automationServiceOrigin,
            _ => l.automationLocalOrigin,
          }),
          if (origin['route'] case final AutomationJson route) ...[
            Text(route['name'] as String? ?? l.automationRoute),
            SelectableText(routeText(route)),
          ],
        ],
      ]),
      card(context, 'targets', Icons.checklist, l.automationTargets, [
        fact(context, l.automationReviewInventorySnapshot, review['inventory_digest'] ?? l.automationResultMissing,
          id: 'automation-review-inventory-digest'),
        Align(
          alignment: Alignment.centerLeft,
          child: boundedBadge(context,
            id: 'automation-review-target-count',
            label: l.automationReviewTargetCount(targets.length.toString()),
          ),
        ),
        const SizedBox(height: 8),
        for (final target in targets)
          Builder(
            builder: (context) {
              final route = target['host_id'] == null
                  ? null
                  : routes.where((r) => r['host_id'] == target['host_id']).firstOrNull;
              final alias = target['alias'] is String && (target['alias'] as String).isNotEmpty
                  ? target['alias'] as String : l.automationResultMissing;
              final username = target['username'] is String && (target['username'] as String).isNotEmpty
                  ? target['username'] as String : l.automationResultMissing;
              final port = target['port'] is int && (target['port'] as int) > 0 && (target['port'] as int) <= 65535
                  ? target['port'].toString() : l.automationResultMissing;
              return Padding(
                padding: const EdgeInsets.symmetric(vertical: 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      route?['name'] as String? ?? target['alias'] as String,
                      style: Theme.of(context).textTheme.labelLarge,
                    ),
                    Wrap(spacing: 12, runSpacing: 4, children: [
                      SelectableText('${l.automationTargetAlias}: $alias'),
                      SelectableText('${l.hostEditorEffectiveUsername}: $username'),
                      SelectableText('${l.hostEditorEffectivePort}: $port'),
                    ]),
                    SelectableText(
                      route == null ? '${target['username']}@${target['address']}:${target['port']}' : routeText(route),
                    ),
                  ],
                ),
              );
            },
          ),
        if (review['inventory_groups'] case final AutomationJson groups)
          for (final entry in groups.entries)
            fact(
              context,
              '${l.automationGroups} · ${entry.key}',
              (entry.value as Map)['hosts'] is List
                  ? ((entry.value as Map)['hosts'] as List).join(', ')
                  : '${(entry.value as Map)['selected_count']} / ${(entry.value as Map)['host_count']}', // l10n-ignore: selected and total numeric host counts
            ),
      ]),
      card(context, 'limits', Icons.tune, l.automationReviewLimitsTitle, [
        fact(context, l.automationTimeout, policy['timeout_seconds'], id: 'automation-review-timeout'),
        fact(context, l.automationReviewParallelism, policy['forks'], id: 'automation-review-forks'),
        AutomationReviewPrivileges(review: review),
        const SizedBox(height: 8),
        Text(policy['cancel_on_lock'] == true ? l.automationCancelOnLock : l.automationKeepRunningOnLock),
        fact(
          context,
          l.automationReviewExpires,
          DateFormat.yMd(l.localeName).add_Hms().format(expiry),
          id: 'automation-review-expiry',
        ),
      ]),
    ];
    return LayoutBuilder(
      builder: (context, box) {
        if (box.maxWidth < 760) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (var i = 0; i < cards.length; i++) ...[if (i > 0) const SizedBox(height: 16), cards[i]],
            ],
          );
        }
        return Column(
          children: [
            for (var i = 0; i < cards.length; i += 2) ...[
              if (i > 0) const SizedBox(height: 16),
              IntrinsicHeight(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Expanded(child: cards[i]),
                    const SizedBox(width: 16),
                    Expanded(child: cards[i + 1]),
                  ],
                ),
              ),
            ],
          ],
        );
      },
    );
  }
}

/// Metadata only. Arbitrary JSON values have no reliable public/secret
/// classification, so no value/ref JSON or unsafe reveal path is constructed.
class AutomationReviewVariables extends StatefulWidget {
  const AutomationReviewVariables({super.key, required this.parameters});
  final Object? parameters;
  @override
  State<AutomationReviewVariables> createState() => _AutomationReviewVariablesState();
}
class _AutomationReviewVariablesState extends State<AutomationReviewVariables> {
  int page = 0;
  static const pageSize = 20;
  @override
  void didUpdateWidget(AutomationReviewVariables oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.parameters, widget.parameters)) page = 0;
  }
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final parameters = widget.parameters;
    final vars = parameters is Map ? parameters['extra_vars'] : null;
    final names = vars is Map && vars.keys.every((key) => key is String)
      ? (vars.keys.cast<String>().toList()..sort()) : null;
    final last = names == null || names.isEmpty ? 0 : (names.length - 1) ~/ pageSize;
    final selected = page.clamp(0, last);
    int? count(Object? value) => value is List && value.every((item) => item is String) ? value.length : null;
    final tags = parameters is Map ? count(parameters['tags']) : null;
    final skip = parameters is Map ? count(parameters['skip_tags']) : null;
    final diff = parameters is Map && parameters['diff'] is bool ? parameters['diff'] as bool : null;
    final bool? vault = parameters is! Map ? null : !parameters.containsKey('vault') ? false :
      parameters['vault'] is Map ? true : null;
    final identifier = RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,127}$');
    final opaque = RegExp(r'^(?:[0-9a-f]{32}|[0-9a-f]{64})$', caseSensitive: false);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      InfoBanner(message: l.automationReviewValuesPrivate),
      const SizedBox(height: 8),
      if (names == null) Text(l.automationResultMissing)
      else if (names.isEmpty) Text(l.automationReviewNoVariables)
      else ...[
        Text(l.automationReviewVariableCount(names.length.toString())),
        for (var index = selected * pageSize; index < names.length && index < (selected + 1) * pageSize; index++)
          Padding(key: ValueKey('automation-reviewed-variable-$index'),
            padding: const EdgeInsets.symmetric(vertical: 5),
            child: Row(children: [
              Expanded(child: Text(identifier.firstMatch(names[index])?.end == names[index].length && !opaque.hasMatch(names[index])
                ? names[index] : l.automationReviewVariableNamePrivate)),
              const SizedBox(width: 12),
              Text(l.automationReviewValuePrivate),
            ])),
        if (last > 0) Wrap(crossAxisAlignment: WrapCrossAlignment.center, children: [
          TextButton(key: const ValueKey('automation-reviewed-variables-previous'),
            onPressed: selected > 0 ? () => setState(() => page = selected - 1) : null,
            child: Text(l.automationInventoryPrevious)),
          Text('${selected + 1} / ${last + 1}'),
          TextButton(key: const ValueKey('automation-reviewed-variables-next'),
            onPressed: selected < last ? () => setState(() => page = selected + 1) : null,
            child: Text(l.automationInventoryNext)),
        ]),
      ],
      Wrap(spacing: 12, runSpacing: 4, children: [
        Text('${l.automationTags}: ${tags?.toString() ?? l.automationResultMissing}'),
        Text('${l.automationSkipTags}: ${skip?.toString() ?? l.automationResultMissing}'),
      ]),
      CheckboxListTile(value: diff, tristate: true, onChanged: null, title: Text(l.automationDiff)),
      CheckboxListTile(value: vault, tristate: true, onChanged: null, title: Text(l.automationVaultUse)),
    ]);
  }
}

/// Frozen Native login accounts only; this does not classify dynamic become,
/// included roles or shell commands and cannot grant their privileges.
class AutomationReviewPrivileges extends StatefulWidget {
  const AutomationReviewPrivileges({super.key, required this.review});
  final AutomationJson review;
  @override
  State<AutomationReviewPrivileges> createState() => _AutomationReviewPrivilegesState();
}
class _AutomationReviewPrivilegesState extends State<AutomationReviewPrivileges> {
  int page = 0;
  static const pageSize = 20;
  @override
  void didUpdateWidget(AutomationReviewPrivileges oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.review, widget.review)) page = 0;
  }
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final raw = widget.review['privilege_review'];
    final targets = widget.review['targets'];
    final accounts = raw is Map ? raw['accounts'] : null;
    final status = raw is Map && raw['escalation'] == 'not_verified'
      ? l.automationReviewNotVerified : l.automationResultMissing;
    final matched = <({String alias, String username})>[];
    final targetsById = <String, Map<Object?, Object?>>{};
    var targetsKnown = targets is List;
    if (targets is List) {
      for (final target in targets) {
        if (target is! Map || target['host_id'] is! String ||
            targetsById.containsKey(target['host_id'])) {
          targetsKnown = false;
          break;
        }
        targetsById[target['host_id'] as String] = target;
      }
    }
    final seen = <String>{};
    var known = accounts is List && targetsKnown && accounts.length == targetsById.length;
    if (known) {
      for (final account in accounts) {
        if (account is! Map || account.length != 2 || account['host_id'] is! String || account['username'] is! String) {
          known = false;
          break;
        }
        final user = account['username'] as String;
        final host = account['host_id'] as String;
        final target = targetsById[host];
        if (target == null || target['username'] != user || !seen.add(host) ||
            user.isEmpty || user.length > 256 || user.runes.any((r) => r < 32 || r == 127)) {
          known = false;
          break;
        }
        matched.add((alias: target['alias'] is String ? target['alias'] as String : l.automationResultMissing,
          username: user));
      }
    }
    final last = !known || matched.isEmpty ? 0 : (matched.length - 1) ~/ pageSize;
    final selected = page.clamp(0, last);
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Text(l.automationReviewPrivileges, style: Theme.of(context).textTheme.labelLarge),
      const SizedBox(height: 6),
      Text('${l.automationReviewEscalation}: $status', key: const ValueKey('automation-reviewed-escalation')),
      Text(l.automationReviewPrivilegesUnknown),
      const SizedBox(height: 6),
      Text(l.automationReviewAccounts, style: Theme.of(context).textTheme.labelMedium),
      if (!known) Text(l.automationResultMissing)
      else for (final account in matched.skip(selected * pageSize).take(pageSize))
        Text('${account.alias}: ${account.username}'),
      if (last > 0) Wrap(crossAxisAlignment: WrapCrossAlignment.center, children: [
        TextButton(key: const ValueKey('automation-reviewed-accounts-previous'),
          onPressed: selected > 0 ? () => setState(() => page = selected - 1) : null,
          child: Text(l.automationInventoryPrevious)),
        Text('${selected + 1} / ${last + 1}'),
        TextButton(key: const ValueKey('automation-reviewed-accounts-next'),
          onPressed: selected < last ? () => setState(() => page = selected + 1) : null,
          child: Text(l.automationInventoryNext)),
      ]),
      AutomationReviewDeclarations(summary: raw is Map ? raw['declarations'] : null),
    ]);
  }
}


/// Optional guest-reported hints only. No effective-rights or approval claim.
class AutomationReviewDeclarations extends StatefulWidget {
  const AutomationReviewDeclarations({super.key, this.summary});
  final Object? summary;
  @override
  State<AutomationReviewDeclarations> createState() => _AutomationReviewDeclarationsState();
}
class _AutomationReviewDeclarationsState extends State<AutomationReviewDeclarations> {
  int page = 0;
  static const size = 20;
  static const areas = {'play', 'pre_tasks', 'tasks', 'post_tasks', 'handlers'};
  static const branches = {'block', 'rescue', 'always'};
  static const methods = {'sudo', 'su', 'doas', 'runas', 'pfexec', 'ksu', 'machinectl', 'pbrun', 'pmrun', 'dzdo'};
  static const fields = {'absent', 'literal_hidden', 'dynamic', 'unknown'};
  @override
  void didUpdateWidget(AutomationReviewDeclarations oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.summary, widget.summary)) page = 0;
  }
  List<Map<Object?, Object?>>? rows() {
    final raw = widget.summary;
    if (raw is! Map || raw.length != 5 || raw['version'] is! int || raw['version'] != 1 || raw['source'] != 'ansible_declared_v1' || raw['truncated'] is! bool) return null;
    final records = raw['records'], unresolved = raw['unresolved'];
    const reasons = {'runtime_overrides', 'roles', 'task_includes', 'shape'};
    if (records is! List || records.length > 64 || unresolved is! List || unresolved.length > 4 ||
        !unresolved.contains('runtime_overrides') || unresolved.toSet().length != unresolved.length ||
        unresolved.any((reason) => !reasons.contains(reason))) {
      return null;
    }
    final result = <Map<Object?, Object?>>[];
    const keys = {'play', 'area', 'indices', 'branches', 'become', 'user', 'method', 'knownMethod'};
    for (final row in records) {
      if (row is! Map || row.length != keys.length || row.keys.any((key) => !keys.contains(key))) return null;
      final play = row['play'], indices = row['indices'], path = row['branches'];
      if (play is! int || play < 1 || play > 4096 || !areas.contains(row['area']) ||
          !{'absent', 'enabled', 'disabled', 'dynamic', 'unknown'}.contains(row['become']) ||
          !fields.contains(row['user']) || !fields.contains(row['method']) ||
          row['knownMethod'] != null && (!methods.contains(row['knownMethod']) || row['method'] != 'literal_hidden') ||
          indices is! List || indices.length > 16 || indices.any((i) => i is! int || i < 1 || i > 4096) ||
          path is! List || path.length != (indices.isEmpty ? 0 : indices.length - 1) ||
          path.any((branch) => !branches.contains(branch)) || (row['area'] == 'play') != indices.isEmpty) {
        return null;
      }
      result.add(row);
    }
    return result;
  }
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final records = rows();
    final last = records == null || records.isEmpty ? 0 : (records.length - 1) ~/ size;
    final selected = page.clamp(0, last);
    String field(Object? value) => switch (value) {
      'absent' => l.automationReviewDeclaredAbsent,
      'enabled' => l.automationReviewDeclaredEnabled,
      'disabled' => l.automationReviewDeclaredDisabled,
      'dynamic' => l.automationReviewDeclaredDynamic,
      'literal_hidden' => l.automationReviewValuePrivate,
      _ => l.commonUnknown,
    };
    return ExpansionTile(
      key: const ValueKey('automation-review-declarations'),
      title: Text(l.automationReviewDeclaredTitle),
      children: [
        InfoBanner(message: l.automationReviewDeclaredHelp),
        if (records == null) Text(l.commonUnknown)
        else ...[
          if ((widget.summary as Map)['truncated'] == true) InfoBanner(tone: BannerTone.warning, message: l.automationReviewDeclaredTruncated),
          if (records.isEmpty) Text(l.automationReviewDeclaredEmpty),
          for (var index = selected * size; index < records.length && index < (selected + 1) * size; index++)
            Builder(builder: (context) {
              final row = records[index];
              final indices = (row['indices'] as List).cast<int>();
              final path = (row['branches'] as List).cast<String>();
              final area = row['area'] as String;
              final position = [area, if (indices.isNotEmpty) indices.first.toString(),
                for (var i = 0; i < path.length; i++) '${path[i]} ${indices[i + 1]}'].join(' / ');
              final method = row['knownMethod'] as String? ?? field(row['method']);
              return Padding(key: ValueKey('automation-declaration-$index'),
                padding: const EdgeInsets.symmetric(vertical: 8), child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(l.automationReviewDeclaredSite(row['play'].toString(), position)),
                  Text(l.automationReviewDeclaredFlags(field(row['become']), field(row['user']), method)),
                ]));
            }),
          if (last > 0) Wrap(crossAxisAlignment: WrapCrossAlignment.center, children: [
            TextButton(key: const ValueKey('automation-declarations-previous'),
              onPressed: selected > 0 ? () => setState(() => page = selected - 1) : null, child: Text(l.automationInventoryPrevious)),
            Text('${selected + 1} / ${last + 1}'),
            TextButton(key: const ValueKey('automation-declarations-next'),
              onPressed: selected < last ? () => setState(() => page = selected + 1) : null, child: Text(l.automationInventoryNext)),
          ]),
        ],
      ],
    );
  }
}
