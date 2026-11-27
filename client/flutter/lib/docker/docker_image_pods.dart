import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/docker/docker_kube_plan.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:material_ui/material_ui.dart';

enum PodImageMatch { digest, tag }

/// Runtime IDs can be content IDs or registry digests, while a spec tag is mutable.
PodImageMatch? matchPodImage(DockerImage image, KubeJson pod) {
  String digest(String ref) => ref.split('://').last.split('@').last;
  final digests = {image.id, ...image.digests.map(digest), ...image.tags.where((t) => t.contains('@')).map(digest)};
  // Also match exact private/local registry references that cannot be used by the deployment wizard.
  String canonical(String ref) => DockerKubeTarget.dockerImage(ref) ?? ref;
  final tags = image.tags.map(canonical).toSet();
  final spec = pod['spec'] as KubeJson?, status = pod['status'] as KubeJson?;
  var tagMatch = false;
  for (final (definitions, states) in const [
    ('containers', 'containerStatuses'),
    ('initContainers', 'initContainerStatuses'),
    ('ephemeralContainers', 'ephemeralContainerStatuses'),
  ]) {
    for (final c in (status?[states] as List? ?? const []).whereType<KubeJson>()) {
      final id = c['imageID'] as String? ?? '';
      if (id.isNotEmpty && digests.contains(digest(id))) return PodImageMatch.digest;
    }
    for (final c in (spec?[definitions] as List? ?? const []).whereType<KubeJson>()) {
      final ref = c['image'] as String? ?? '';
      if (ref.contains('@') || ref.startsWith('sha256:')) {
        if (digests.contains(digest(ref))) return PodImageMatch.digest;
      } else if (ref.isNotEmpty && tags.contains(canonical(ref))) {
        tagMatch = true;
      }
    }
  }
  return tagMatch ? PodImageMatch.tag : null;
}

class DockerImagePods extends StatefulWidget {
  const DockerImagePods({required this.service, required this.image, super.key});
  final KubeService service;
  final DockerImage image;

  @override
  State<DockerImagePods> createState() => _DockerImagePodsState();
}

class _DockerImagePodsState extends State<DockerImagePods> {
  late final Future<List<KubeConnection>> contexts = _loadContexts();

  Future<List<KubeConnection>> _loadContexts() async {
    final connections = await widget.service.inventory();
    if (mounted && widget.service is KubeBrowserService && connections.length == 1) {
      check(connections.single, false);
    }
    return connections;
  }

  KubeConnection? selected;
  bool allNamespaces = false;
  int generation = 0;
  Future<List<(String, PodImageMatch)>>? results;

  void check(KubeConnection connection, bool all) {
    final version = ++generation;
    setState(() {
      selected = connection;
      allNamespaces = all;
      results = _read(connection, all, version);
    });
  }

  Future<List<(String, PodImageMatch)>> _read(KubeConnection connection, bool all, int version) async {
    final browser = widget.service as KubeBrowserService;
    final session = await browser.connect(connection.id);
    final found = <(String, PodImageMatch)>[];
    try {
      const pods = KubeResourceKind(
        group: '',
        version: 'v1',
        resource: 'pods',
        kind: 'Pod',
        namespaced: true,
        verbs: ['list'],
      );
      String? token;
      final seen = <String>{};
      while (mounted && version == generation) {
        final page = await browser.listResources(
          session.id,
          pods,
          namespace: all ? null : connection.namespace,
          continueToken: token,
          limit: 100,
          includeObjects: true,
        );
        for (final pod in page.items) {
          if (pod.object['spec'] is! Map) throw const FormatException();
          final match = matchPodImage(widget.image, pod.object);
          if (match != null) {
            final name = '${pod.namespace ?? connection.namespace}/${pod.name}';
            final phase = (pod.object['status'] as KubeJson?)?['phase'] as String?;
            found.add((phase == null || phase.isEmpty ? name : '$name · $phase', match));
          }
        }
        token = page.continueToken;
        if (token == null || token.isEmpty) break;
        if (!seen.add(token)) throw const FormatException();
      }
      return found..sort((a, b) => a.$1.compareTo(b.$1));
    } finally {
      await browser.disconnect(session.id);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(l.dockerImagePodCheck, style: GlassTokens.of(context).typography.bodyEmph),
        const SizedBox(height: 6),
        FutureBuilder<List<KubeConnection>>(
          future: contexts,
          builder: (context, snapshot) {
            if (snapshot.hasError) return Text(l.kubeLoadFailed);
            if (!snapshot.hasData) return const LinearProgressIndicator();
            final connections = snapshot.requireData;
            if (connections.isEmpty) return Text(l.dockerImageNoContexts);
            if (widget.service is! KubeBrowserService) return Text(l.kubeRequestFailed);
            return Wrap(
              spacing: 6,
              runSpacing: 4,
              children: [
                GlassSelect<KubeConnection?>(
                  value: selected,
                  semanticLabel: l.dockerKubeContext,
                  items: [
                    if (selected == null) GlassSelectItem(value: null, label: l.dockerKubeContext),
                    for (final c in connections) GlassSelectItem(value: c, label: c.name),
                  ],
                  onChanged: (c) {
                    if (c != null) check(c, false);
                  },
                ),
                if (selected case final c?) ...[
                  GlassSelect<bool>(
                    value: allNamespaces,
                    items: [
                      GlassSelectItem(value: false, label: c.namespace),
                      GlassSelectItem(value: true, label: l.kubeAllNamespaces),
                    ],
                    onChanged: (v) => check(c, v),
                  ),
                  GlassIconButton(
                    icon: Icons.refresh_rounded,
                    tooltip: l.dockerRefresh,
                    onPressed: () => check(c, allNamespaces),
                  ),
                ],
              ],
            );
          },
        ),
        if (results case final future?)
          FutureBuilder<List<(String, PodImageMatch)>>(
            future: future,
            builder: (context, snapshot) {
              if (snapshot.connectionState != ConnectionState.done) return const LinearProgressIndicator();
              if (snapshot.hasError) return Text(l.kubeRequestFailed);
              if (snapshot.requireData.isEmpty) return Text(l.dockerImageNoPods);
              return SelectableText(
                snapshot.requireData
                    .map(
                      (r) => '${r.$1} · ${r.$2 == PodImageMatch.digest ? l.dockerImagePodDigest : l.dockerImagePodTag}',
                    )
                    .join('\n'),
              );
            },
          ),
      ],
    );
  }
}
