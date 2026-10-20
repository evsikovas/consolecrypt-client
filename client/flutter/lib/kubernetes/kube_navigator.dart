import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_connection_status.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:material_ui/material_ui.dart';

/// Resource sections of the desktop navigator, in display order.
enum KubeNavSection { workloads, network, config, storage, access, cluster, gitops, custom }

const _builtinGroups = {
  '',
  'apps',
  'batch',
  'autoscaling',
  'policy',
  'networking.k8s.io',
  'discovery.k8s.io',
  'rbac.authorization.k8s.io',
  'storage.k8s.io',
  'apiextensions.k8s.io',
  'apiregistration.k8s.io',
  'admissionregistration.k8s.io',
  'coordination.k8s.io',
  'scheduling.k8s.io',
  'node.k8s.io',
  'certificates.k8s.io',
  'flowcontrol.apiserver.k8s.io',
  'events.k8s.io',
  'resource.k8s.io',
  'internal.apiserver.k8s.io',
  'storagemigration.k8s.io',
};

/// Display order and plural labels of well-known kinds. Kubernetes kind names
/// are API identifiers and stay untranslated.
const _known = <String, (KubeNavSection, int, String)>{
  'Pod': (KubeNavSection.workloads, 0, 'Pods'), // l10n-ignore: Kubernetes API kind.
  'Deployment': (KubeNavSection.workloads, 1, 'Deployments'), // l10n-ignore: Kubernetes API kind.
  'StatefulSet': (KubeNavSection.workloads, 2, 'StatefulSets'), // l10n-ignore: Kubernetes API kind.
  'DaemonSet': (KubeNavSection.workloads, 3, 'DaemonSets'), // l10n-ignore: Kubernetes API kind.
  'ReplicaSet': (KubeNavSection.workloads, 4, 'ReplicaSets'), // l10n-ignore: Kubernetes API kind.
  'Job': (KubeNavSection.workloads, 5, 'Jobs'), // l10n-ignore: Kubernetes API kind.
  'CronJob': (KubeNavSection.workloads, 6, 'CronJobs'), // l10n-ignore: Kubernetes API kind.
  'ReplicationController': (KubeNavSection.workloads, 7, 'ReplicationControllers'), // l10n-ignore: Kubernetes API kind.
  'Service': (KubeNavSection.network, 0, 'Services'), // l10n-ignore: Kubernetes API kind.
  'Ingress': (KubeNavSection.network, 1, 'Ingresses'), // l10n-ignore: Kubernetes API kind.
  'IngressClass': (KubeNavSection.network, 2, 'IngressClasses'), // l10n-ignore: Kubernetes API kind.
  'NetworkPolicy': (KubeNavSection.network, 3, 'NetworkPolicies'), // l10n-ignore: Kubernetes API kind.
  'Endpoints': (KubeNavSection.network, 4, 'Endpoints'), // l10n-ignore: Kubernetes API kind.
  'EndpointSlice': (KubeNavSection.network, 5, 'EndpointSlices'), // l10n-ignore: Kubernetes API kind.
  'ConfigMap': (KubeNavSection.config, 0, 'ConfigMaps'), // l10n-ignore: Kubernetes API kind.
  'Secret': (KubeNavSection.config, 1, 'Secrets'), // l10n-ignore: Kubernetes API kind.
  'HorizontalPodAutoscaler': (KubeNavSection.config, 2, 'HPA'), // l10n-ignore: Kubernetes API kind.
  'PodDisruptionBudget': (KubeNavSection.config, 3, 'PDB'), // l10n-ignore: Kubernetes API kind.
  'ResourceQuota': (KubeNavSection.config, 4, 'ResourceQuotas'), // l10n-ignore: Kubernetes API kind.
  'LimitRange': (KubeNavSection.config, 5, 'LimitRanges'), // l10n-ignore: Kubernetes API kind.
  'PriorityClass': (KubeNavSection.config, 6, 'PriorityClasses'), // l10n-ignore: Kubernetes API kind.
  'RuntimeClass': (KubeNavSection.config, 7, 'RuntimeClasses'), // l10n-ignore: Kubernetes API kind.
  'MutatingWebhookConfiguration': (KubeNavSection.config, 8, 'MutatingWebhooks'), // l10n-ignore: Kubernetes API kind.
  'ValidatingWebhookConfiguration': (
    KubeNavSection.config,
    9,
    'ValidatingWebhooks',
  ), // l10n-ignore: Kubernetes API kind.
  'PersistentVolumeClaim': (KubeNavSection.storage, 0, 'PVC'), // l10n-ignore: Kubernetes API kind.
  'PersistentVolume': (KubeNavSection.storage, 1, 'PV'), // l10n-ignore: Kubernetes API kind.
  'StorageClass': (KubeNavSection.storage, 2, 'StorageClasses'), // l10n-ignore: Kubernetes API kind.
  'VolumeAttachment': (KubeNavSection.storage, 3, 'VolumeAttachments'), // l10n-ignore: Kubernetes API kind.
  'CSIDriver': (KubeNavSection.storage, 4, 'CSIDrivers'), // l10n-ignore: Kubernetes API kind.
  'CSINode': (KubeNavSection.storage, 5, 'CSINodes'), // l10n-ignore: Kubernetes API kind.
  'ServiceAccount': (KubeNavSection.access, 0, 'ServiceAccounts'), // l10n-ignore: Kubernetes API kind.
  'Role': (KubeNavSection.access, 1, 'Roles'), // l10n-ignore: Kubernetes API kind.
  'RoleBinding': (KubeNavSection.access, 2, 'RoleBindings'), // l10n-ignore: Kubernetes API kind.
  'ClusterRole': (KubeNavSection.access, 3, 'ClusterRoles'), // l10n-ignore: Kubernetes API kind.
  'ClusterRoleBinding': (KubeNavSection.access, 4, 'ClusterRoleBindings'), // l10n-ignore: Kubernetes API kind.
  'Node': (KubeNavSection.cluster, 0, 'Nodes'), // l10n-ignore: Kubernetes API kind.
  'Namespace': (KubeNavSection.cluster, 1, 'Namespaces'), // l10n-ignore: Kubernetes API kind.
  'Event': (KubeNavSection.cluster, 2, 'Events'), // l10n-ignore: Kubernetes API kind.
  'CustomResourceDefinition': (KubeNavSection.cluster, 3, 'CRD'), // l10n-ignore: Kubernetes API kind.
  'APIService': (KubeNavSection.cluster, 4, 'APIServices'), // l10n-ignore: Kubernetes API kind.
  'Lease': (KubeNavSection.cluster, 5, 'Leases'), // l10n-ignore: Kubernetes API kind.
};

bool kubeIsGitOpsGroup(String group) => group.endsWith('.toolkit.fluxcd.io') || group == 'argoproj.io';

/// Section of a discovered kind in the navigator.
KubeNavSection kubeNavSection(KubeResourceKind kind) {
  if (kubeIsGitOpsGroup(kind.group)) return KubeNavSection.gitops;
  if (!_builtinGroups.contains(kind.group)) return KubeNavSection.custom;
  return _known[kind.kind]?.$1 ?? KubeNavSection.cluster;
}

/// Plural label of a kind ("Pods", "PVC", or the CRD kind itself).
String kubeKindLabel(KubeResourceKind kind) {
  if (_builtinGroups.contains(kind.group)) {
    if (_known[kind.kind] case final known?) return known.$3;
  }
  return kind.kind;
}

/// Kinds shown in the navigator: listable, without the duplicate
/// events.k8s.io Event when the core one exists.
List<KubeResourceKind> kubeNavigableKinds(List<KubeResourceKind> kinds) {
  final coreEvents = kinds.any((kind) => kind.group.isEmpty && kind.kind == 'Event');
  return kinds
      .where((kind) => kind.canList && !kind.resource.contains('/'))
      .where((kind) => !(coreEvents && kind.group == 'events.k8s.io'))
      .where((kind) => kind.group != 'metrics.k8s.io')
      .toList();
}

/// Desktop resource navigator (left pane of the cluster workspace): saved
/// clusters with their status, overview pages and collapsible groups of
/// discovered kinds. Custom resources are grouped by API group with a filter.
class KubeNavigator extends StatefulWidget {
  const KubeNavigator({
    required this.connections,
    required this.currentConnectionId,
    required this.statuses,
    required this.kinds,
    required this.view,
    required this.selectedKindId,
    required this.onSelectKind,
    required this.onSelectView,
    required this.onSelectContext,
    super.key,
    this.favouriteKinds = const {},
    this.gitOpsAlerts = 0,
    this.hasHistory = false,
    this.hasInsights = false,
  });

  final List<KubeConnection> connections;
  final String currentConnectionId;
  final Map<String, KubeConnectionStatus> statuses;
  final List<KubeResourceKind> kinds;
  final String view;
  final String? selectedKindId;
  final Set<String> favouriteKinds;
  final int gitOpsAlerts;
  final bool hasHistory, hasInsights;
  final ValueChanged<KubeResourceKind> onSelectKind;
  final ValueChanged<String> onSelectView;
  final ValueChanged<KubeConnection>? onSelectContext;

  @override
  State<KubeNavigator> createState() => _KubeNavigatorState();
}

class _KubeNavigatorState extends State<KubeNavigator> {
  final _collapsed = <String>{'section:${KubeNavSection.storage.name}', 'section:${KubeNavSection.access.name}'};
  final _expandedGroups = <String>{};
  final _filter = TextEditingController();
  String _query = '';

  @override
  void dispose() {
    _filter.dispose();
    super.dispose();
  }

  String _title(AppLocalizations l, KubeNavSection section) => switch (section) {
    KubeNavSection.workloads => l.kubeGroupWorkloads,
    KubeNavSection.network => l.kubeGroupNetwork,
    KubeNavSection.config => l.kubeGroupConfig,
    KubeNavSection.storage => l.kubeGroupStorage,
    KubeNavSection.access => l.kubeGroupAccess,
    KubeNavSection.cluster => l.kubeNavCluster,
    KubeNavSection.gitops => l.kubeGitOps,
    KubeNavSection.custom => l.kubeNavCustom,
  };

  void _toggle(String id) => setState(() => _collapsed.contains(id) ? _collapsed.remove(id) : _collapsed.add(id));

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final kinds = kubeNavigableKinds(widget.kinds);
    final bySection = <KubeNavSection, List<KubeResourceKind>>{};
    for (final kind in kinds) {
      bySection.putIfAbsent(kubeNavSection(kind), () => []).add(kind);
    }
    for (final entry in bySection.entries) {
      entry.value.sort((a, b) {
        final left = _known[a.kind]?.$2 ?? 100, right = _known[b.kind]?.$2 ?? 100;
        return left == right ? a.kind.compareTo(b.kind) : left.compareTo(right);
      });
    }
    final favourites = kinds.where((kind) => widget.favouriteKinds.contains(kind.id)).toList()
      ..sort((a, b) => kubeKindLabel(a).compareTo(kubeKindLabel(b)));
    final children = <Widget>[
      if (widget.connections.isNotEmpty) ...[
        _header(context, 'clusters', l.kubeClusters),
        if (!_collapsed.contains('clusters'))
          for (final connection in widget.connections) _cluster(context, connection),
      ],
      if (widget.hasInsights) ...[
        _header(context, 'overview', l.kubeNavOverview),
        if (!_collapsed.contains('overview')) ...[
          _item(
            context,
            key: 'kube-view-problems',
            label: l.kubeProblems,
            icon: Icons.warning_amber_rounded,
            selected: widget.view == 'problems',
            onTap: () => widget.onSelectView('problems'),
          ),
          if (widget.hasHistory)
            _item(
              context,
              key: 'kube-view-history',
              label: l.kubeHistory,
              icon: Icons.history,
              selected: widget.view == 'history',
              onTap: () => widget.onSelectView('history'),
            ),
          _item(
            context,
            key: 'kube-view-helm',
            label: l.kubeNavHelmReleases,
            icon: Icons.sailing_outlined,
            selected: widget.view == 'helm',
            onTap: () => widget.onSelectView('helm'),
          ),
        ],
      ],
      if (favourites.isNotEmpty) ...[
        _header(context, 'favourites', l.kubeFavorites),
        if (!_collapsed.contains('favourites'))
          for (final kind in favourites) _kind(context, kind),
      ],
      for (final section in KubeNavSection.values.where((s) => s != KubeNavSection.custom))
        if (bySection[section]?.isNotEmpty == true ||
            (section == KubeNavSection.gitops && widget.hasInsights && _hasGitOps)) ...[
          _header(
            context,
            'section:${section.name}',
            _title(l, section),
            badge: section == KubeNavSection.gitops && widget.gitOpsAlerts > 0 ? '${widget.gitOpsAlerts}' : null,
          ),
          if (!_collapsed.contains('section:${section.name}')) ...[
            if (section == KubeNavSection.gitops && widget.hasInsights)
              _item(
                context,
                key: 'kube-view-gitops',
                label: l.kubeNavOverview,
                icon: Icons.account_tree_outlined,
                selected: widget.view == 'gitops',
                onTap: () => widget.onSelectView('gitops'),
              ),
            for (final kind in bySection[section] ?? const <KubeResourceKind>[]) _kind(context, kind),
          ],
        ],
      if (bySection[KubeNavSection.custom]?.isNotEmpty == true) ..._custom(context, bySection[KubeNavSection.custom]!),
    ];
    return ContentSurface(
      key: const ValueKey('kube-navigator'),
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s6),
      child: ScrollEdgeEffect(
        bottom: true,
        extent: 12,
        child: ListView(
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
          children: children,
        ),
      ),
    );
  }

  bool get _hasGitOps => widget.kinds.any((kind) => kubeIsGitOpsGroup(kind.group));

  List<Widget> _custom(BuildContext context, List<KubeResourceKind> kinds) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final query = _query.trim().toLowerCase();
    final groups = <String, List<KubeResourceKind>>{};
    for (final kind in kinds) {
      if (query.isNotEmpty && !'${kind.group} ${kind.kind}'.toLowerCase().contains(query)) continue;
      groups.putIfAbsent(kind.group, () => []).add(kind);
    }
    final names = groups.keys.toList()..sort();
    const id = 'section:custom'; // l10n-ignore: internal section id
    return [
      _header(context, id, l.kubeNavCustom, badge: '${kinds.length}'),
      if (!_collapsed.contains(id)) ...[
        Padding(
          padding: const EdgeInsets.fromLTRB(GlassSpacing.s4, GlassSpacing.s2, GlassSpacing.s4, GlassSpacing.s6),
          child: GlassField(
            key: const ValueKey('kube-nav-crd-filter'),
            controller: _filter,
            search: true,
            placeholder: l.kubeNavCrdFilter,
            onChanged: (value) => setState(() => _query = value),
          ),
        ),
        for (final group in names) ...[
          _row(
            context,
            key: 'kube-nav-group-$group',
            leading: Icon(
              _expandedGroups.contains(group) || query.isNotEmpty ? Icons.expand_more : Icons.chevron_right,
              size: 16,
              color: tokens.secondaryLabel,
            ),
            label: group,
            trailing: '${groups[group]!.length}',
            selected: false,
            tooltip: group,
            onTap: () => setState(
              () => _expandedGroups.contains(group) ? _expandedGroups.remove(group) : _expandedGroups.add(group),
            ),
          ),
          if (_expandedGroups.contains(group) || query.isNotEmpty)
            for (final kind in groups[group]!..sort((a, b) => a.kind.compareTo(b.kind)))
              _kind(context, kind, indent: 22),
        ],
      ],
    ];
  }

  Widget _header(BuildContext context, String id, String title, {String? badge}) {
    final tokens = GlassTokens.of(context);
    final collapsed = _collapsed.contains(id);
    return Padding(
      padding: const EdgeInsets.only(top: GlassSpacing.s6),
      child: GlassInteractive(
        key: ValueKey('kube-nav-header-$id'),
        onPressed: () => _toggle(id),
        semanticLabel: title,
        builder: (context, state) => DecoratedBox(
          decoration: ShapeDecoration(
            color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
            shape: GlassRadii.shape(tokens.radii.row),
          ),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s6, GlassSpacing.s4),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    title.toUpperCase(),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.caption.copyWith(
                      color: tokens.secondaryLabel,
                      fontWeight: FontWeight.w600,
                      letterSpacing: 0.6,
                    ),
                  ),
                ),
                if (badge != null) ...[
                  Text(badge, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                  const SizedBox(width: GlassSpacing.s4),
                ],
                Icon(collapsed ? Icons.chevron_right : Icons.expand_more, size: 14, color: tokens.palette.tertiary),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _cluster(BuildContext context, KubeConnection connection) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final current = connection.id == widget.currentConnectionId;
    final status = current ? KubeConnectionState.available : widget.statuses[connection.id]?.state;
    final color = switch (status) {
      KubeConnectionState.available => p.success,
      KubeConnectionState.checking => p.warning,
      KubeConnectionState.failed || KubeConnectionState.forbidden => p.danger,
      null => p.tertiary,
    };
    final host = Uri.tryParse(connection.server)?.authority ?? connection.server;
    return GlassInteractive(
      key: ValueKey('kube-nav-context-${connection.id}'),
      selected: current,
      semanticLabel: connection.name,
      onPressed: current || widget.onSelectContext == null ? null : () => widget.onSelectContext!(connection),
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: current
              ? tokens.sidebarSelection
              : state.hovered
              ? tokens.surfaces.fillHover
              : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s4),
          child: Row(
            children: [
              StatusDot(color: color),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      connection.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: (current ? tokens.typography.bodyEmph : tokens.typography.body).copyWith(
                        color: current ? p.accent : p.label,
                      ),
                    ),
                    if (host.isNotEmpty)
                      Text(
                        host,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                      ),
                  ],
                ),
              ),
              if (connection.production)
                const GlassBadge(label: 'PROD', tone: GlassTone.danger, dense: true), // l10n-ignore: environment marker
            ],
          ),
        ),
      ),
    );
  }

  Widget _kind(BuildContext context, KubeResourceKind kind, {double indent = 0}) => _item(
    context,
    key: 'kube-nav-kind-${kind.id}',
    label: kubeKindLabel(kind),
    selected: widget.view == 'resources' && widget.selectedKindId == kind.id,
    tooltip: '${kind.kind} · ${kind.apiVersion}', // l10n-ignore: Kubernetes API identifiers.
    indent: indent,
    onTap: () => widget.onSelectKind(kind),
  );

  Widget _item(
    BuildContext context, {
    required String key,
    required String label,
    required bool selected,
    required VoidCallback onTap,
    IconData? icon,
    String? tooltip,
    double indent = 0,
  }) {
    final tokens = GlassTokens.of(context);
    return _row(
      context,
      key: key,
      leading: icon == null
          ? null
          : Icon(icon, size: 16, color: selected ? tokens.palette.accent : tokens.secondaryLabel),
      label: label,
      selected: selected,
      tooltip: tooltip,
      indent: indent,
      onTap: onTap,
    );
  }

  Widget _row(
    BuildContext context, {
    required String key,
    required String label,
    required bool selected,
    required VoidCallback onTap,
    Widget? leading,
    String? trailing,
    String? tooltip,
    double indent = 0,
  }) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    Widget row = GlassInteractive(
      key: ValueKey(key),
      selected: selected,
      semanticLabel: label,
      onPressed: onTap,
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: selected
              ? tokens.sidebarSelection
              : state.hovered
              ? tokens.surfaces.fillHover
              : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: SizedBox(
          height: 28,
          child: Padding(
            padding: EdgeInsetsDirectional.only(start: GlassSpacing.s8 + indent, end: GlassSpacing.s8),
            child: Row(
              children: [
                if (leading != null) ...[leading, const SizedBox(width: GlassSpacing.s8)],
                Expanded(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: (selected ? tokens.typography.bodyEmph : tokens.typography.body).copyWith(
                      color: selected ? p.accent : p.label,
                    ),
                  ),
                ),
                if (trailing != null)
                  Text(trailing, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
              ],
            ),
          ),
        ),
      ),
    );
    if (tooltip != null) row = Tooltip(message: tooltip, waitDuration: const Duration(milliseconds: 600), child: row);
    return row;
  }
}
