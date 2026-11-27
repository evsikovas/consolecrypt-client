import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_image_pods.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:material_ui/material_ui.dart';

typedef _Row = ({DockerImage image, String? tag});

enum _Action { details, remove }

enum _Usage { all, used, unused, unknown }

_Usage _usage(DockerImage image) => switch (image.consumers?.isEmpty) {
  true => _Usage.unused,
  false => _Usage.used,
  null => _Usage.unknown,
};

String _usageLabel(AppLocalizations l, _Usage usage) => switch (usage) {
  _Usage.all => l.dockerImageAll,
  _Usage.used => l.dockerImageUsed,
  _Usage.unused => l.dockerImageUnused,
  _Usage.unknown => l.dockerImageUsageUnknown,
};

typedef _ImageKey = (String, String?);
_ImageKey _rowKey(_Row row) => (row.image.id, row.tag);

class DockerImageList extends StatefulWidget {
  const DockerImageList({required this.service, required this.session, this.query = '', this.kubernetes, super.key});
  final DockerObservationService service;
  final DockerSessionInfo session;
  final String query;
  final KubeService? kubernetes;
  @override
  State<DockerImageList> createState() => DockerImageListState();
}

class DockerImageListState extends State<DockerImageList> {
  late Future<List<_Row>> _rows = _load();
  final _selected = <_ImageKey>{};
  _Usage _filter = _Usage.all;
  bool _oldest = false, _deleting = false;

  @override
  void didUpdateWidget(DockerImageList oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.query != widget.query) _selected.clear();
    if (oldWidget.session.id != widget.session.id) {
      _selected.clear();
      _rows = _load();
    }
  }

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
    _selected.clear();
    _rows = _load();
  });

  Future<void> _act(_Action action, _Row row) async {
    if (!mounted) return;
    final l = context.l10n, tag = row.tag;
    if (action == _Action.details) {
      final session = widget.session.id;
      final _Row fresh;
      try {
        final images = await widget.service.images(session);
        if (!mounted || widget.session.id != session) return;
        final image = images.where((i) => i.id == row.image.id && (tag == null || i.tags.contains(tag))).firstOrNull;
        if (image == null) {
          refresh();
          return;
        }
        fresh = (image: image, tag: tag);
      } catch (error) {
        if (mounted) showSnack(context, dockerErrorText(l, error), error: true);
        return;
      }
      final img = fresh.image;
      final fields = [
        (
          l.dockerImageUsage,
          [
            _usageLabel(l, _usage(img)),
            if (img.consumers case final consumers?)
              for (final c in consumers)
                [
                  c.name,
                  dockerStateLabel(l, c.state),
                  if (c.pod != null) 'Pod ${c.namespace == null ? c.pod : "${c.namespace}/${c.pod}"}',
                ].join(' · '),
          ].join('\n'),
        ),
        (l.wslcImageID, img.id),
        if (img.tags.isNotEmpty) (l.tagsLabel, img.tags.join('\n')),
        if (img.digests.isNotEmpty) (l.dbFolderReferences, img.digests.join('\n')),
        if (img.size case final bytes?) (l.sftpColumnSize, formatBytes(l, bytes)),
        if (img.created case final date?) (l.dockerCreated, formatDateTime(l, date)),
      ];
      final remove = await showAppDialog<bool>(
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
                  if (widget.kubernetes case final kube?) DockerImagePods(service: kube, image: img),
                ],
              ),
            ),
            secondaryActions: [
              GlassButton(
                label: l.commonDelete,
                icon: Icons.delete_outline,
                style: GlassButtonStyle.destructiveQuiet,
                onPressed: _usage(img) == _Usage.unused && !_deleting ? () => closeDialog(ctx, true) : null,
              ),
            ],
            primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog(ctx, false)),
          );
        },
      );
      if (remove == true && mounted && widget.session.id == session) await _delete([fresh]);
      return;
    }
    await _delete([row]);
  }

  Future<void> _delete(List<_Row> targets) async {
    if (_deleting || targets.isEmpty || targets.any((r) => _usage(r.image) != _Usage.unused)) return;
    final l = context.l10n, session = widget.session.id;
    setState(() => _deleting = true);
    String reference(_Row r) => r.tag ?? r.image.digests.firstOrNull ?? r.image.id;
    final single = targets.length == 1 ? targets.first : null;
    final untag = single != null && single.tag != null && single.image.tags.length > 1;
    try {
      final confirmed = await showAppDialog<bool>(
        context,
        builder: (ctx) => GlassDialog(
          title: single == null
              ? l.dockerImageDeleteSelected(targets.length)
              : untag
              ? l.wslcRemoveImageTag
              : l.wslcDeleteImageInventoryPages,
          icon: Icons.warning_rounded,
          iconTone: GlassTone.danger,
          content: SingleChildScrollView(
            child: Text(
              untag
                  ? l.wslcOnlyTheTagWillBeRemovedTheLayers(single.tag!)
                  : '${targets.map(reference).join("\n")}\n\n${l.wslcThisCannotBeUndoneYouWillNeedTo}',
            ),
          ),
          secondaryActions: [
            GlassButton(
              key: const ValueKey('confirm-cancel'),
              label: l.commonCancel,
              autofocus: true,
              onPressed: () => closeDialog(ctx, false),
            ),
          ],
          primaryAction: GlassButton.destructive(
            key: const ValueKey('confirm-ok'),
            label: l.commonDelete,
            onPressed: () => closeDialog(ctx, true),
          ),
        ),
      );
      if (confirmed != true || !mounted || widget.session.id != session) return;
      final issues = <String>[];
      for (final row in targets) {
        if (!mounted || widget.session.id != session) return;
        try {
          final ok = await widget.service.removeImage(session, row.image.id, tag: row.tag);
          if (!ok) issues.add('${reference(row)} — ${l.dockerRemovalInUse}');
        } catch (e) {
          issues.add('${reference(row)} — ${dockerErrorText(l, e)}');
        }
      }
      if (!mounted || widget.session.id != session) return;
      refresh();
      if (issues.isNotEmpty) {
        await showAppDialog<void>(
          context,
          builder: (ctx) => GlassDialog(
            title: l.dockerImageRetained,
            content: SingleChildScrollView(child: SelectableText(issues.join('\n'))),
            primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(ctx)),
          ),
        );
      }
    } finally {
      if (mounted) setState(() => _deleting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, t = GlassTokens.of(context);
    final meta = t.typography.caption.copyWith(color: t.secondaryLabel, fontSize: 10.5, height: 1.35);
    return FutureBuilder<List<_Row>>(
      future: _rows,
      builder: (context, snapshot) {
        if (snapshot.hasError) return Center(child: Text(dockerErrorText(l, snapshot.error!)));
        if (!snapshot.hasData) return const Center(child: CircularProgressIndicator());
        final query = widget.query.trim().toLowerCase();
        final ordered = _oldest ? snapshot.requireData.reversed : snapshot.requireData;
        final rows = ordered
            .where(
              (r) =>
                  (_filter == _Usage.all || _usage(r.image) == _filter) &&
                  [r.tag ?? '', r.image.id, ...r.image.digests].any((s) => s.toLowerCase().contains(query)),
            )
            .toList();
        final eligible = rows.where((r) => _usage(r.image) == _Usage.unused).toList();
        final selected = eligible.where((r) => _selected.contains(_rowKey(r))).toList();
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
              child: Wrap(
                spacing: 8,
                runSpacing: 4,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  GlassSelect<_Usage>(
                    key: const ValueKey('docker-image-filter'),
                    value: _filter,
                    semanticLabel: l.dockerImageUsage,
                    items: [
                      for (final usage in _Usage.values) GlassSelectItem(value: usage, label: _usageLabel(l, usage)),
                    ],
                    onChanged: (v) => setState(() {
                      _filter = v;
                      _selected.clear();
                    }),
                  ),
                  GlassSelect<bool>(
                    value: _oldest,
                    items: [
                      GlassSelectItem(value: false, label: l.kubeSortAge),
                      GlassSelectItem(value: true, label: l.dockerImageOldest),
                    ],
                    onChanged: (v) => setState(() => _oldest = v),
                  ),
                  GlassIconButton(
                    icon: Icons.select_all_rounded,
                    tooltip: l.dockerImageSelectUnused,
                    onPressed: _deleting || eligible.isEmpty
                        ? null
                        : () => setState(() {
                            if (selected.length == eligible.length) {
                              _selected.clear();
                            } else {
                              _selected.addAll(eligible.map(_rowKey));
                            }
                          }),
                  ),
                  if (selected.isNotEmpty)
                    GlassButton(
                      label: l.dockerImageDeleteSelected(selected.length),
                      icon: Icons.delete_outline,
                      onPressed: _deleting ? null : () => _delete(selected),
                    ),
                ],
              ),
            ),
            Expanded(
              child: rows.isEmpty
                  ? Center(child: Text(snapshot.requireData.isEmpty ? l.wslcNoImagesYet : l.dockerNoMatches))
                  : ListView.separated(
                      padding: const EdgeInsets.symmetric(vertical: 4),
                      separatorBuilder: (_, _) => const SizedBox(height: 4),
                      itemCount: rows.length,
                      itemBuilder: (context, i) {
                        final row = rows[i], img = row.image, date = img.created;
                        final usage = _usage(img);
                        final color = switch (usage) {
                          _Usage.used => t.palette.success,
                          _Usage.unused => t.palette.warning,
                          _ => t.secondaryLabel,
                        };
                        final actions = [
                          GlassMenuItem(value: _Action.details, label: l.dockerDetails, icon: Icons.info_outline),
                          if (usage == _Usage.unused && !_deleting) ...[
                            const GlassMenuDivider<_Action>(),
                            GlassMenuItem(
                              value: _Action.remove,
                              label: l.wslcDeleteImage,
                              icon: Icons.delete_outline,
                              destructive: true,
                            ),
                          ],
                        ];
                        final days = date == null ? 0 : DateTime.now().difference(date).inDays;
                        final age = date == null
                            ? '—'
                            : days >= 7
                            ? l.dockerImageWeeksAgo(days ~/ 7)
                            : formatRelative(l, date);
                        Future<void> menu(Offset point) async {
                          final action = await showGlassMenu(
                            context: context,
                            anchor: point & Size.zero,
                            entries: actions,
                          );
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
                                    Checkbox(
                                      value: _selected.contains(_rowKey(row)) && usage == _Usage.unused,
                                      onChanged: usage != _Usage.unused || _deleting
                                          ? null
                                          : (v) => setState(() {
                                              if (v == true) {
                                                _selected.add(_rowKey(row));
                                              } else {
                                                _selected.remove(_rowKey(row));
                                              }
                                            }),
                                    ),

                                    const SizedBox(width: 7),
                                    Expanded(
                                      flex: 5,
                                      child: Tooltip(
                                        message: row.tag ?? img.digests.firstOrNull ?? img.id,
                                        child: Column(
                                          crossAxisAlignment: CrossAxisAlignment.start,
                                          children: [
                                            Text(
                                              img.label(row.tag),
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                              style: t.typography.callout.copyWith(fontSize: 12, height: 1.35),
                                            ),
                                            Text(
                                              _usageLabel(l, usage),
                                              style: meta.copyWith(color: color),
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                            ),
                                          ],
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
                    ),
            ),
          ],
        );
      },
    );
  }
}
