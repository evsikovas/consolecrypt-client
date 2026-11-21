import 'dart:async';

import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart' show confirmDanger;
import 'package:consolecrypt/wslc/features/inventory/image_dialogs.dart' show imageRef;
import 'package:consolecrypt/wslc/features/topology/topology_page.dart' show MapModel, NodeKind;
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/usage.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';


/// Removal order: containers first, so their images and volumes become free.
enum CleanupKind { container, image, volume, network }

/// Something that can be removed to free disk space or tidy the runtime.
class CleanupItem {
  const CleanupItem(this.kind, this.key, this.label, {this.detail, this.bytes, String? sizeKey})
    : sizeKey = sizeKey ?? key;
  final CleanupKind kind;

  /// Container ID, image reference, volume or network name.
  final String key;
  final String label;
  final String? detail;
  final double? bytes;

  /// Tags of one image share its size; it is counted once.
  final String sizeKey;

  String get id => '${kind.name}:$key';
}

/// Stopped containers and images, volumes and user networks nothing uses.
List<CleanupItem> cleanupCandidates({
  required List<ContainerSummary> containers,
  required List<ContainerLinks> links,
  required List<ImageSummary> images,
  required List<VolumeSummary> volumes,
  required List<NetworkSummary> networks,
}) {
  final model = MapModel.build(links: links, images: images, volumes: volumes, networks: networks);
  final unused = {
    for (final col in model.columns)
      for (final n in col)
        if (n.unused) (n.kind, n.key),
  };
  return [
    for (final c in containers)
      if (c.state == ContainerState.exited || c.state == ContainerState.created)
        CleanupItem(
          CleanupKind.container,
          c.id,
          c.name ?? shortId(c.id),
          detail: [c.image, c.statusDisplay ?? c.rawState].nonNulls.join(' · '),
        ),
    for (final i in images)
      // The runtime's own count guards against a link the map could not match.
      if (unused.contains((NodeKind.image, i.id)) && (i.containers ?? 0) == 0)
        CleanupItem(
          CleanupKind.image,
          imageRef(i),
          i.repository == null ? shortId(i.id) : '${i.repository}:${i.tag ?? '<none>'}',
          detail: i.createdAt == null ? null : 'создан ${i.createdAt!.split(' ').first}',
          bytes: parseSize(i.sizeDisplay),
          sizeKey: i.id,
        ),
    for (final v in volumes)
      if (unused.contains((NodeKind.volume, v.name)))
        CleanupItem(CleanupKind.volume, v.name, v.name, detail: v.driver, bytes: parseSize(v.sizeDisplay)),
    for (final n in networks)
      if (n.name != null && unused.contains((NodeKind.network, n.name)))
        CleanupItem(CleanupKind.network, n.name!, n.name!, detail: n.driver),
  ];
}

/// Upper bound of what removing [items] frees; layers shared between images make it smaller.
double reclaimable(Iterable<CleanupItem> items) {
  final sizes = <String, double>{
    for (final i in items)
      if (i.bytes != null) '${i.kind.name}:${i.sizeKey}': i.bytes!,
  };
  return sizes.values.fold(0, (a, b) => a + b);
}

class CleanupPage extends StatefulWidget {
  const CleanupPage({super.key, required this.controller});
  final DeskController controller;
  @override
  State<CleanupPage> createState() => _CleanupPageState();
}

class _CleanupPageState extends State<CleanupPage> {
  late final api = WslcApiScope.of(context);

  List<ContainerLinks>? links;
  String? error;
  final selected = <String>{};
  List<ContainerSummary> _seen = const [];

  /// Items done so far while a cleanup runs; null when idle.
  (int, int)? progress;

  @override
  void initState() {
    super.initState();
    unawaited(load());
  }

  @override
  void didUpdateWidget(covariant CleanupPage old) {
    super.didUpdateWidget(old);
    if (progress == null && !identical(_seen, widget.controller.containers)) {
      unawaited(load());
    }
  }

  Future<void> load() async {
    _seen = widget.controller.containers;
    try {
      final result = await api.containerLinks();
      if (mounted) {
        setState(() {
          links = result;
          error = null;
        });
      }
    } catch (e) {
      if (mounted) setState(() => error = errorText(e));
    }
  }

  Future<void> _remove(CleanupItem i) => switch (i.kind) {
    CleanupKind.container => api.removeContainer(containerId: i.key, force: false),
    CleanupKind.image => api.removeImage(imageId: i.key),
    CleanupKind.volume => api.removeVolume(volumeName: i.key),
    CleanupKind.network => api.removeNetwork(networkName: i.key),
  };

  Future<void> clean(List<CleanupItem> items) async {
    final c = widget.controller;
    final chosen = [
      for (final i in items)
        if (selected.contains(i.id)) i,
    ]..sort((a, b) => a.kind.index.compareTo(b.kind.index));
    if (chosen.isEmpty || !c.canMutate) return;
    final count = {for (final k in CleanupKind.values) k: chosen.where((i) => i.kind == k).length};
    final ok = await confirmDanger(
      context,
      icon: Icons.cleaning_services_rounded,
      title: 'Удалить выбранное (${chosen.length})?',
      message: [
        for (final MapEntry(key: k, value: n) in count.entries)
          if (n > 0) '${_titles[k]}: $n',
        '',
        if (count[CleanupKind.volume]! > 0) 'Данные выбранных томов будут потеряны.',
        'Удаление нельзя отменить.',
      ].join('\n'),
      action: 'Удалить',
    );
    if (!ok || !mounted) return;
    final failures = <String>[];
    setState(() => progress = (0, chosen.length));
    for (final (n, item) in chosen.indexed) {
      try {
        await _remove(item);
        selected.remove(item.id);
      } catch (e) {
        failures.add('${item.label}: ${errorText(e)}');
      }
      if (mounted) setState(() => progress = (n + 1, chosen.length));
    }
    await c.refreshSections(inventorySections);
    if (!mounted) return;
    setState(() => progress = null);
    unawaited(load());
    final removed = chosen.length - failures.length;
    if (failures.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('Удалено: $removed')));
      return;
    }
    await showWslcDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Удалено $removed из ${chosen.length}'),
        content: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480, maxHeight: 320),
          child: SingleChildScrollView(child: WslcSelectableText(failures.join('\n\n'), minLines: 1)),
        ),
        actions: [FilledButton(onPressed: () => Navigator.pop(context), child: const Text('Закрыть'))],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final p = DeskPalette.of(context);
    final data = links;
    if (data == null) {
      return Panel(
        child: EmptyState(
          icon: Icons.cleaning_services_outlined,
          loading: error == null,
          title: error == null ? 'Ищем, что можно удалить…' : 'Не удалось собрать список',
          hint: error,
        ),
      );
    }
    final items = cleanupCandidates(
      containers: c.containers,
      links: data,
      images: c.images,
      volumes: c.volumes,
      networks: c.networks,
    );
    // Drop selections whose resource is gone.
    selected.retainAll(items.map((i) => i.id));
    final chosen = items.where((i) => selected.contains(i.id));
    final size = reclaimable(chosen);
    final running = progress;
    final total = reclaimable(items);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Pill(
              items.isEmpty
                  ? 'Всё чисто'
                  : total > 0
                  ? 'Можно освободить до ${formatBytes(total, decimal: true)}'
                  : 'Найдено: ${items.length}',
              color: items.isEmpty ? p.success : p.accent,
              icon: Icons.savings_outlined,
            ),
            const Spacer(),
            if (running != null) ...[
              SizedBox(
                width: 160,
                child: LinearProgressIndicator(value: running.$1 / running.$2, borderRadius: BorderRadius.circular(4)),
              ),
              const SizedBox(width: 10),
              Text('${running.$1} из ${running.$2}', style: TextStyle(color: p.muted, fontSize: 12.5)),
              const SizedBox(width: 14),
            ],
            FilledButton.icon(
              onPressed: chosen.isEmpty || running != null || !c.canMutate ? null : () => clean(items),
              icon: const Icon(Icons.cleaning_services_rounded, size: 18),
              label: Text(
                chosen.isEmpty
                    ? 'Очистить выбранное'
                    : 'Очистить выбранное (${chosen.length}'
                          '${size > 0 ? ' · ${formatBytes(size, decimal: true)}' : ''})',
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Expanded(
          child: Panel(
            child: items.isEmpty
                ? const EmptyState(
                    icon: Icons.task_alt_rounded,
                    title: 'Чистить нечего',
                    hint: 'Нет остановленных контейнеров, а образы, тома и сети используются',
                  )
                : ListView(
                    padding: const EdgeInsets.symmetric(vertical: 8),
                    children: [
                      for (final kind in CleanupKind.values)
                        if (items.any((i) => i.kind == kind))
                          _Group(
                            kind: kind,
                            items: [
                              for (final i in items)
                                if (i.kind == kind) i,
                            ],
                            selected: selected,
                            enabled: running == null,
                            onChanged: (ids, value) =>
                                setState(() => value ? selected.addAll(ids) : selected.removeAll(ids)),
                          ),
                    ],
                  ),
          ),
        ),
      ],
    );
  }
}

const _titles = {
  CleanupKind.container: 'Остановленные контейнеры',
  CleanupKind.image: 'Неиспользуемые образы',
  CleanupKind.volume: 'Неиспользуемые тома',
  CleanupKind.network: 'Пустые сети',
};

const _hints = {
  CleanupKind.container: 'Удаляется файловая система контейнера; именованные тома остаются.',
  CleanupKind.image: 'Ни один контейнер не создан из этих образов; их можно загрузить заново.',
  CleanupKind.volume:
      'Не подключены ни к одному контейнеру. Данные будут потеряны — сделайте бэкап, если сомневаетесь.',
  CleanupKind.network: 'Пользовательские сети без подключённых контейнеров.',
};

IconData _icon(CleanupKind k) => switch (k) {
  CleanupKind.container => Icons.view_in_ar_rounded,
  CleanupKind.image => Icons.layers_rounded,
  CleanupKind.volume => Icons.storage_rounded,
  CleanupKind.network => Icons.hub_rounded,
};

class _Group extends StatelessWidget {
  const _Group({
    required this.kind,
    required this.items,
    required this.selected,
    required this.enabled,
    required this.onChanged,
  });
  final CleanupKind kind;
  final List<CleanupItem> items;
  final Set<String> selected;
  final bool enabled;
  final void Function(Iterable<String> ids, bool value) onChanged;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final ids = items.map((i) => i.id);
    final picked = ids.where(selected.contains).length;
    final size = reclaimable(items);
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 4, 16, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Checkbox(
                tristate: true,
                value: picked == 0
                    ? false
                    : picked == items.length
                    ? true
                    : null,
                onChanged: enabled ? (_) => onChanged(ids, picked < items.length) : null,
              ),
              Icon(_icon(kind), size: 18, color: p.accent),
              const SizedBox(width: 8),
              Text(
                '${_titles[kind]} · ${items.length}',
                style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
              ),
              const Spacer(),
              if (size > 0) Text(formatBytes(size, decimal: true), style: TextStyle(color: p.muted, fontSize: 12.5)),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(left: 40, bottom: 4),
            child: Text(_hints[kind]!, style: TextStyle(color: p.faint, fontSize: 12)),
          ),
          for (final i in items)
            Material(
              type: MaterialType.transparency,
              child: InkWell(
                borderRadius: BorderRadius.circular(8),
                onTap: enabled ? () => onChanged([i.id], !selected.contains(i.id)) : null,
                child: Padding(
                  padding: const EdgeInsets.only(left: 28),
                  child: Row(
                    children: [
                      Checkbox(
                        value: selected.contains(i.id),
                        onChanged: enabled ? (v) => onChanged([i.id], v ?? false) : null,
                      ),
                      Expanded(
                        child: Text.rich(
                          TextSpan(
                            children: [
                              TextSpan(
                                text: i.label,
                                style: const TextStyle(fontWeight: FontWeight.w600),
                              ),
                              if (i.detail != null && i.detail!.isNotEmpty)
                                TextSpan(
                                  text: '   ${i.detail}',
                                  style: TextStyle(color: p.faint),
                                ),
                            ],
                          ),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontSize: 13),
                        ),
                      ),
                      if (i.bytes != null && i.bytes! > 0)
                        Text(formatBytes(i.bytes!, decimal: true), style: TextStyle(color: p.muted, fontSize: 12.5)),
                    ],
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
