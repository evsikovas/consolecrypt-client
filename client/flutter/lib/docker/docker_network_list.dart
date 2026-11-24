import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:material_ui/material_ui.dart';

class DockerNetworkList extends StatefulWidget {
  const DockerNetworkList({
    required this.service,
    required this.session,
    required this.query,
    required this.containers,
    super.key,
  });
  final DockerObservationService service;
  final DockerSessionInfo session;
  final String query;
  final List<DockerContainer> containers;
  @override
  State<DockerNetworkList> createState() => DockerNetworkListState();
}

class DockerNetworkListState extends State<DockerNetworkList> {
  late var _data = widget.service.networks(widget.session.id);
  void refresh() => setState(() {
    _data = widget.service.networks(widget.session.id);
  });
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final caption = tokens.typography.caption.copyWith(color: tokens.secondaryLabel, fontSize: 10.5, height: 1.35);
    final titleStyle = tokens.typography.bodyEmph.copyWith(fontSize: 12, height: 1.35);
    return FutureBuilder<List<DockerJson>>(
      future: _data,
      builder: (context, snapshot) {
        if (snapshot.hasError) return Center(child: Text(dockerErrorText(l, snapshot.error!)));
        if (!snapshot.hasData) return const Center(child: CircularProgressIndicator());
        final query = widget.query.trim().toLowerCase();
        final rows =
            snapshot.data!
                .where(
                  (row) => [
                    'name',
                    'id',
                    'driver',
                    'scope',
                    'ranges',
                  ].any((k) => (row[k] ?? '').toString().toLowerCase().contains(query)),
                )
                .toList()
              ..sort((a, b) => (a['name'] as String).compareTo(b['name'] as String));
        if (rows.isEmpty) return Center(child: Text(l.dockerNetworkEmpty));
        return ListView.separated(
          padding: const EdgeInsets.symmetric(vertical: 4),
          separatorBuilder: (_, _) => const SizedBox(height: 4),
          itemCount: rows.length,
          itemBuilder: (context, i) {
            final row = rows[i], name = row['name'] as String;
            final members = widget.containers.where((c) => c.networks.containsKey(name)).toList()
              ..sort((a, b) => a.name.compareTo(b.name));
            final ranges = (row['ranges'] as List? ?? const []).whereType<Map<String, dynamic>>();
            final summary = [
              row['driver'],
              row['scope'],
              for (final r in ranges) r['subnet'],
            ].whereType<String>().join(' · ');
            final shape = GlassRadii.shape(tokens.radii.card);
            return ContentSurface(
              key: ValueKey((widget.session.id, row['id'])),
              kind: ContentSurfaceKind.inset,
              radius: tokens.radii.card,
              padding: EdgeInsets.zero,
              child: ExpansionTile(
                dense: true,
                tilePadding: const EdgeInsets.symmetric(horizontal: 12),
                minTileHeight: Theme.of(context).materialTapTargetSize == MaterialTapTargetSize.padded ? 48 : 34,
                shape: shape,
                collapsedShape: shape,
                clipBehavior: Clip.antiAlias,
                leading: Icon(Icons.hub_outlined, size: 15, color: tokens.palette.accent),
                title: Wrap(
                  spacing: 10,
                  runSpacing: 2,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(name, style: titleStyle),
                    if (summary.isNotEmpty) Text(summary, style: caption),
                  ],
                ),
                expandedCrossAxisAlignment: CrossAxisAlignment.start,
                childrenPadding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
                children: [
                  const Divider(height: 9),
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: SelectableText("${l.dockerNetworkId}  ${row['id'] ?? ''}", style: caption),
                  ),
                  for (final range in ranges)
                    if (range['gateway'] case final String gw) SelectableText(l.wslcGateway(gw), style: caption),
                  const SizedBox(height: 6),
                  Text(
                    '${l.dockerNetworkContainers} (${members.length})',
                    style: caption.copyWith(color: tokens.palette.label, fontWeight: FontWeight.w600),
                  ),
                  for (final c in members)
                    SelectableText(
                      [c.name, if (c.networks[name]?.isNotEmpty == true) c.networks[name]!].join(' · '),
                      style: caption,
                    ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}
