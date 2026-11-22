import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart' show confirmDanger;
import 'package:consolecrypt/wslc/features/inventory/image_dialogs.dart';
import 'package:consolecrypt/wslc/features/inventory/resource_dialogs.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Shared layout: search field and a virtualized list inside a panel.
class _InventoryPage<T> extends StatefulWidget {
  const _InventoryPage({
    super.key,
    required this.controller,
    required this.section,
    required this.items,
    required this.searchText,
    required this.hint,
    required this.icon,
    required this.emptyTitle,
    required this.itemBuilder,
    this.spacing = 0,
    this.actions = const [],
  });
  final DeskController controller;
  final DeskSection section;
  final List<T> items;
  final String Function(T) searchText;
  final String hint;
  final IconData icon;
  final String emptyTitle;
  final Widget Function(BuildContext, T) itemBuilder;
  final double spacing;

  /// Buttons shown next to the search field.
  final List<Widget> actions;

  @override
  State<_InventoryPage<T>> createState() => _InventoryPageState<T>();
}

class _InventoryPageState<T> extends State<_InventoryPage<T>> {
  final search = TextEditingController();

  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final query = search.text.trim().toLowerCase();
    final rows = widget.items
        .where((r) => query.isEmpty || widget.searchText(r).toLowerCase().contains(query))
        .toList(growable: false);
    final empty = EmptyState(
      icon: widget.icon,
      loading: c.ready && !c.loaded.contains(widget.section) && c.error == null,
      title: !c.ready
          ? context.l10n.wslcNotConnectedToWSLc
          : widget.items.isEmpty
          ? widget.emptyTitle
          : context.l10n.notesNothingFound,
      hint: !c.ready ? context.l10n.wslcCheckTheConnectionInSettings : null,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: SearchField(controller: search, hint: widget.hint, onChanged: (_) => setState(() {})),
            ),
            for (final a in widget.actions) ...[const SizedBox(width: 10), a],
          ],
        ),
        const SizedBox(height: 14),
        Expanded(
          child: widget.spacing > 0
              ? rows.isEmpty
                    ? Panel(child: empty)
                    : ListView.separated(
                        itemCount: rows.length,
                        separatorBuilder: (_, _) => SizedBox(height: widget.spacing),
                        itemBuilder: (context, i) => widget.itemBuilder(context, rows[i]),
                      )
              : Panel(
                  child: rows.isEmpty
                      ? empty
                      : ListView.separated(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          itemCount: rows.length,
                          separatorBuilder: (_, _) => Divider(
                            height: 1,
                            indent: 16,
                            endIndent: 16,
                            color: DeskPalette.of(context).border.withValues(alpha: .6),
                          ),
                          itemBuilder: (context, i) => widget.itemBuilder(context, rows[i]),
                        ),
                ),
        ),
      ],
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({
    required this.icon,
    required this.color,
    required this.title,
    required this.subtitle,
    this.badges = const [],
    this.meta,
    this.onInspect,
    this.onDelete,
    this.menu,
    this.deleteTooltip,
    this.busy = false,
  });
  final IconData icon;
  final Color color;
  final String title;
  final Widget subtitle;
  final List<Widget> badges;
  final String? meta;
  final VoidCallback? onInspect;
  final VoidCallback? onDelete;

  /// Extra actions shown after delete.
  final Widget? menu;
  final String? deleteTooltip;
  final bool busy;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
      child: Row(
        children: [
          GradientIcon(icon, color: color, size: 38),
          const SizedBox(width: 14),
          Expanded(
            flex: 5,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
                      ),
                    ),
                    for (final b in badges) ...[const SizedBox(width: 8), b],
                  ],
                ),
                const SizedBox(height: 3),
                subtitle,
              ],
            ),
          ),
          if (meta != null)
            Expanded(
              flex: 2,
              child: Text(
                meta!,
                textAlign: TextAlign.right,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: p.muted, fontSize: 12.5),
              ),
            ),
          const SizedBox(width: 12),
          if (onInspect != null)
            IconButton(
              tooltip: context.l10n.dockerDetails,
              onPressed: onInspect,
              icon: const Icon(Icons.data_object_rounded, size: 20),
            ),
          if (busy)
            const Padding(
              padding: EdgeInsets.all(11),
              child: SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
            )
          else
            IconButton(
              tooltip: deleteTooltip ?? context.l10n.commonDelete,
              color: p.danger,
              onPressed: onDelete,
              icon: const Icon(Icons.delete_outline_rounded, size: 20),
            ),
          ?menu,
        ],
      ),
    );
  }
}

Future<void> _confirmAndRemove(
  BuildContext context, {
  required String title,
  required String message,
  required Future<String?> Function() remove,
  required String done,
}) async {
  final ok = await confirmDanger(context, title: title, message: message, action: context.l10n.commonDelete);
  if (!ok || !context.mounted) return;
  final notify = WslcApiScope.notifierOf(context);
  final error = await remove();
  notify(error ?? done, error: error != null);
}

class ImagesPage extends StatelessWidget {
  const ImagesPage({super.key, required this.controller});
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return _InventoryPage<ImageSummary>(
      controller: controller,
      section: DeskSection.images,
      items: controller.images,
      searchText: (r) => '${r.repository} ${r.tag} ${r.id}',
      hint: context.l10n.wslcSearchByRepositoryTagOrID,
      icon: Icons.layers_rounded,
      emptyTitle: context.l10n.wslcNoImagesYet,
      actions: [
        OutlinedButton.icon(
          onPressed: controller.ready ? () => manageRegistries(context, controller) : null,
          icon: const Icon(Icons.vpn_key_outlined, size: 18),
          label: Text(context.l10n.wslcRegistries),
        ),
        OutlinedButton.icon(
          onPressed: controller.canMutate ? () => buildImage(context, controller) : null,
          icon: const Icon(Icons.construction_rounded, size: 18),
          label: Text(context.l10n.wslcBuildImageDialogs),
        ),
        FilledButton.icon(
          onPressed: controller.canMutate ? () => pullImage(context, controller) : null,
          icon: const Icon(Icons.cloud_download_rounded, size: 18),
          label: Text(context.l10n.wslcDownload),
        ),
        ImageArchiveMenu(controller: controller),
      ],
      itemBuilder: (context, r) => _Row(
        icon: Icons.layers_rounded,
        color: p.accentEnd,
        title: r.repository ?? context.l10n.automationResultMissing,
        badges: [
          Pill(r.tag ?? context.l10n.wslcUntagged, color: p.accent),
          if ((r.containers ?? 0) > 0)
            Pill(context.l10n.wslcContainers(r.containers!), color: p.success, icon: Icons.view_in_ar_rounded),
        ],
        subtitle: CopyId(r.id),
        meta: [
          r.sizeDisplay ?? context.l10n.wslcSizeUnknown,
          if (r.createdAt != null) r.createdAt!.split(' ').first,
        ].join('\n'),
        onInspect: () => showInspectDialog(
          context,
          context.l10n.wslcImageInventoryPages(r.repository ?? shortId(r.id), r.tag == null ? '' : ':${r.tag}'),
          () => WslcApiScope.of(context).inspectImage(imageId: r.id, revealEnvironment: false),
        ),
        deleteTooltip: context.l10n.wslcDeleteImage,
        busy: controller.busyIds.contains(imageRef(r)),
        menu: ImageMenu(image: r, controller: controller),
        onDelete: controller.canMutate
            ? () => _confirmAndRemove(
                context,
                title: r.repository == null
                    ? context.l10n.wslcDeleteImageInventoryPages
                    : context.l10n.wslcRemoveImageTag,
                message:
                    '${r.repository ?? context.l10n.wslcUnnamed}:${r.tag ?? context.l10n.wslcUntagged}\n${r.id}\n\n'
                    '${(r.containers ?? 0) > 0 ? context.l10n.wslcTheImageIsUsedByContainerSThe(r.containers!) : ''}'
                    '${r.repository == null ? context.l10n.wslcThisCannotBeUndoneYouWillNeedTo : context.l10n.wslcOnlyTheTagWillBeRemovedTheLayers(imageRef(r))}',
                remove: () => controller.removeImage(imageRef(r)),
                done: context.l10n.wslcImageDeleted,
              )
            : null,
      ),
    );
  }
}

class VolumesPage extends StatelessWidget {
  const VolumesPage({super.key, required this.controller});
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return _InventoryPage<VolumeSummary>(
      controller: controller,
      section: DeskSection.storage,
      items: controller.volumes,
      searchText: (r) => '${r.name} ${r.driver}',
      hint: context.l10n.wslcSearchByVolumeName,
      icon: Icons.storage_rounded,
      emptyTitle: context.l10n.wslcNoVolumesYet,
      actions: [
        FilledButton.icon(
          onPressed: controller.canMutate ? () => createVolume(context, controller) : null,
          icon: const Icon(Icons.add_rounded, size: 18),
          label: Text(context.l10n.wslcCreateVolume),
        ),
        OutlinedButton.icon(
          onPressed: controller.canMutate ? () => restoreVolume(context, controller) : null,
          icon: const Icon(Icons.settings_backup_restore_rounded, size: 18),
          label: Text(context.l10n.sharingRestore),
        ),
      ],
      itemBuilder: (context, r) => _Row(
        icon: Icons.storage_rounded,
        color: p.warning,
        title: r.name,
        subtitle: Text(
          context.l10n.wslcDriver(r.driver ?? context.l10n.automationResultMissing),
          style: TextStyle(color: p.muted, fontSize: 12),
        ),
        meta: r.sizeDisplay ?? context.l10n.wslcSizeUnavailable,
        onInspect: () => showInspectDialog(
          context,
          context.l10n.wslcVolume(r.name),
          () => WslcApiScope.of(context).inspectVolume(volumeName: r.name),
        ),
        deleteTooltip: context.l10n.wslcDeleteVolume,
        menu: VolumeMenu(volume: r.name, controller: controller),
        busy: controller.busyIds.contains('volume:${r.name}'),
        onDelete: controller.canMutate
            ? () => _confirmAndRemove(
                context,
                title: context.l10n.wslcDeleteVolumeInventoryPages,
                message: context.l10n.wslcAllDataInThisVolumeWillBePermanently(r.name),
                remove: () => controller.removeVolume(r.name),
                done: context.l10n.wslcVolumeDeleted(r.name),
              )
            : null,
      ),
    );
  }
}

class NetworksPage extends StatelessWidget {
  const NetworksPage({super.key, required this.controller});
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    String flag(bool? v) => v == null
        ? context.l10n.automationResultMissing
        : v
        ? context.l10n.wslcYes
        : context.l10n.wslcNo;
    return _InventoryPage<NetworkSummary>(
      controller: controller,
      section: DeskSection.networks,
      items: controller.networks,
      searchText: (n) => '${n.name} ${n.id} ${n.driver}',
      hint: context.l10n.wslcSearchByNameIDOrDriver,
      icon: Icons.hub_rounded,
      emptyTitle: context.l10n.wslcNoNetworksFound,
      actions: [
        FilledButton.icon(
          onPressed: controller.canMutate ? () => createNetwork(context, controller) : null,
          icon: const Icon(Icons.add_rounded, size: 18),
          label: Text(context.l10n.wslcCreateNetwork),
        ),
      ],
      spacing: 10,
      itemBuilder: (context, n) => Panel(
        padding: const EdgeInsets.all(18),
        child: Row(
          children: [
            GradientIcon(Icons.hub_rounded, color: p.accent, size: 44),
            const SizedBox(width: 16),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    n.name ?? context.l10n.automationResultMissing,
                    style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w700),
                  ),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      Pill(
                        '${n.driver ?? context.l10n.automationResultMissing} · ${n.scope ?? context.l10n.automationResultMissing}',
                        color: p.accentEnd,
                        icon: Icons.settings_ethernet_rounded,
                      ),
                      Pill('IPv4: ${flag(n.ipv4)}', color: p.success),
                      Pill('IPv6: ${flag(n.ipv6)}', color: n.ipv6 == true ? p.success : p.muted),
                      if (n.internal == true) Pill(context.l10n.wslcInternal, color: p.warning),
                    ],
                  ),
                  const SizedBox(height: 8),
                  CopyId(n.id, label: n.id),
                ],
              ),
            ),
            const SizedBox(width: 12),
            OutlinedButton.icon(
              onPressed: n.name == null
                  ? null
                  : () => showInspectDialog(
                      context,
                      context.l10n.wslcNetwork(n.name!),
                      () => WslcApiScope.of(context).inspectNetwork(networkName: n.name!),
                    ),
              icon: const Icon(Icons.manage_search_rounded, size: 18),
              label: Text(context.l10n.wslcDetails),
            ),
            if (n.name != null && n.name != 'host' && n.name != 'none')
              IconButton(
                tooltip: context.l10n.wslcConnectContainer,
                onPressed: controller.canMutate ? () => connectToNetwork(context, controller, network: n.name) : null,
                icon: const Icon(Icons.add_link_rounded),
              ),
            if (n.name != null && !builtinNetworks.contains(n.name))
              IconButton(
                tooltip: context.l10n.wslcDeleteNetwork,
                color: p.danger,
                onPressed: controller.canMutate
                    ? () => _confirmAndRemove(
                        context,
                        title: context.l10n.wslcDeleteNetworkInventoryPages,
                        message: context.l10n.wslcTheRuntimeWillRefuseToDeleteThisNetwork(n.name!),
                        remove: () => controller.removeNetwork(n.name!),
                        done: context.l10n.wslcNetworkDeleted(n.name!),
                      )
                    : null,
                icon: const Icon(Icons.delete_outline_rounded),
              ),
          ],
        ),
      ),
    );
  }
}
