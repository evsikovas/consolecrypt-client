import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:material_ui/material_ui.dart';

typedef _Row = ({DockerImage image, String? tag});

enum _Action { details, remove }

class DockerImageList extends StatefulWidget {
  const DockerImageList({required this.service, required this.session, this.query = '', super.key});
  final DockerObservationService service;
  final DockerSessionInfo session;
  final String query;
  @override
  State<DockerImageList> createState() => DockerImageListState();
}

class DockerImageListState extends State<DockerImageList> {
  late Future<List<_Row>> _rows = _load();
  final _removing = <String>{};

  Future<List<_Row>> _load() async {
    final images = [...await widget.service.images(widget.session.id)];
    images.sort((a, b) {
      final byDate = (b.created?.millisecondsSinceEpoch ?? 0).compareTo(a.created?.millisecondsSinceEpoch ?? 0);
      return byDate != 0 ? byDate : a.label(a.tags.firstOrNull).compareTo(b.label(b.tags.firstOrNull));
    });
    return [
      for (final img in images)
        if (img.tags.isEmpty) (image: img, tag: null) else for (final tag in img.tags) (image: img, tag: tag),
    ];
  }

  void refresh() => setState(() {
    _rows = _load();
  });

  Future<void> _act(_Action action, _Row row) async {
    if (!mounted) return;
    final l = context.l10n, img = row.image, tag = row.tag;
    if (action == _Action.details) {
      final fields = [
        (l.wslcImageID, img.id),
        if (img.tags.isNotEmpty) (l.tagsLabel, img.tags.join('\n')),
        if (img.digests.isNotEmpty) (l.dbFolderReferences, img.digests.join('\n')),
        if (img.size case final bytes?) (l.sftpColumnSize, formatBytes(l, bytes)),
        if (img.created case final date?) (l.dockerCreated, formatDateTime(l, date)),
      ];
      await showAppDialog<void>(
        context,
        builder: (ctx) {
          final t = GlassTokens.of(ctx);
          return GlassDialog(
            title: img.label(tag),
            content: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final (label, value) in fields)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 10),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(label, style: t.typography.caption.copyWith(color: t.secondaryLabel)),
                          SelectableText(value, style: t.typography.mono.copyWith(fontSize: 11)),
                        ],
                      ),
                    ),
                ],
              ),
            ),
            primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(ctx)),
          );
        },
      );
      return;
    }
    if (!_removing.add(img.id)) return;
    final sessionId = widget.session.id;
    try {
      final untag = img.tags.length > 1 && tag != null;
      final target = tag ?? img.digests.firstOrNull ?? img.id;
      final ok = await showConfirmDialog(
        context,
        title: untag ? l.wslcRemoveImageTag : l.wslcDeleteImageInventoryPages,
        message: untag
            ? l.wslcOnlyTheTagWillBeRemovedTheLayers(tag)
            : '$target\n\n${l.wslcThisCannotBeUndoneYouWillNeedTo}',
        confirmLabel: l.commonDelete,
        destructive: true,
      );
      if (!ok || !mounted || widget.session.id != sessionId) return;
      final removed = await widget.service.removeImage(sessionId, img.id, tag: tag);
      if (mounted) {
        refresh();
        if (!removed) showSnack(context, l.dockerRemovalInUse, error: true);
      }
    } catch (e) {
      if (mounted) showSnack(context, dockerErrorText(l, e), error: true);
    } finally {
      _removing.remove(img.id);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, t = GlassTokens.of(context);
    final meta = t.typography.caption.copyWith(color: t.secondaryLabel, fontSize: 10.5, height: 1.35);
    final actions = [
      GlassMenuItem(value: _Action.details, label: l.dockerDetails, icon: Icons.info_outline),
      const GlassMenuDivider<_Action>(),
      GlassMenuItem(value: _Action.remove, label: l.wslcDeleteImage, icon: Icons.delete_outline, destructive: true),
    ];
    return FutureBuilder<List<_Row>>(
      future: _rows,
      builder: (context, snapshot) {
        if (snapshot.hasError) return Center(child: Text(dockerErrorText(l, snapshot.error!)));
        if (!snapshot.hasData) return const Center(child: CircularProgressIndicator());
        final query = widget.query.trim().toLowerCase();
        final rows = query.isEmpty
            ? snapshot.data!
            : snapshot.data!
                  .where(
                    (r) => [r.tag ?? '', r.image.id, ...r.image.digests].any((s) => s.toLowerCase().contains(query)),
                  )
                  .toList();
        if (rows.isEmpty && query.isNotEmpty) return Center(child: Text(l.dockerNoMatches));
        if (rows.isEmpty) return Center(child: Text(l.wslcNoImagesYet));
        return ListView.separated(
          padding: const EdgeInsets.symmetric(vertical: 4),
          separatorBuilder: (_, _) => const SizedBox(height: 4),
          itemCount: rows.length,
          itemBuilder: (context, i) {
            final row = rows[i], img = row.image, date = img.created;
            final days = date == null ? 0 : DateTime.now().difference(date).inDays;
            final age = date == null
                ? '—'
                : days >= 7
                ? l.dockerImageWeeksAgo(days ~/ 7)
                : formatRelative(l, date);
            Future<void> menu(Offset point) async {
              final action = await showGlassMenu(context: context, anchor: point & Size.zero, entries: actions);
              if (action != null && context.mounted) await _act(action, row);
            }

            return ContentSurface(
              kind: ContentSurfaceKind.inset,
              radius: t.radii.card,
              padding: EdgeInsets.zero,
              child: GestureDetector(
                onSecondaryTapUp: (e) => menu(e.globalPosition),
                onLongPressStart: (e) => menu(e.globalPosition),
                child: InkWell(
                  borderRadius: BorderRadius.circular(t.radii.card),
                  onTap: () => _act(_Action.details, row),
                  child: Padding(
                    padding: const EdgeInsetsDirectional.only(start: 12, end: 4),
                    child: Row(
                      children: [
                        Icon(Icons.layers_outlined, size: 14, color: t.secondaryLabel),
                        const SizedBox(width: 7),
                        Expanded(
                          flex: 5,
                          child: Tooltip(
                            message: row.tag ?? img.digests.firstOrNull ?? img.id,
                            child: Text(
                              img.label(row.tag),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: t.typography.callout.copyWith(fontSize: 12, height: 1.35),
                            ),
                          ),
                        ),
                        const SizedBox(width: 6),
                        Expanded(
                          flex: 2,
                          child: Text(
                            img.size == null ? '—' : formatBytes(l, img.size!),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            textAlign: TextAlign.end,
                            style: meta,
                          ),
                        ),
                        const SizedBox(width: 6),
                        Expanded(
                          flex: 2,
                          child: Text(
                            age,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            textAlign: TextAlign.end,
                            style: meta,
                          ),
                        ),
                        GlassMenuButton<_Action>(
                          entries: actions,
                          onSelected: (a) => _act(a, row),
                          builder: (_, open) => GlassIconButton(
                            icon: Icons.more_horiz,
                            tooltip: l.wslcMoreActions,
                            size: GlassSizes.toolbarButton,
                            iconSize: 14,
                            style: GlassIconButtonStyle.plain,
                            onPressed: open,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            );
          },
        );
      },
    );
  }
}
