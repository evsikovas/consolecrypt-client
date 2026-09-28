import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/sftp/sftp_actions.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Browser toolbar (chrome, LIQUID_GLASS_SPEC §3 layer 3, static): the host
/// capsule (icon, name, SFTP badge, switch host), the actions capsule (local
/// pane, Quick Look, Action menu, Refresh, Transfers / Editing with counts),
/// the search field and Disconnect.
class SftpToolbar extends ConsumerWidget {
  const SftpToolbar({
    required this.searchController,
    required this.searchFocus,
    required this.onQuickLook,
    required this.actionMenuEntries,
    required this.onAction,
    required this.onSwitchHost,
    required this.onDisconnect,
    required this.onSearchEscape,
    super.key,
  });

  final TextEditingController searchController;
  final FocusNode searchFocus;
  final VoidCallback? onQuickLook;

  /// The Action menu for the current selection.
  final List<GlassMenuEntry<SftpAction>> actionMenuEntries;
  final ValueChanged<SftpAction> onAction;
  final VoidCallback onSwitchHost;
  final VoidCallback onDisconnect;
  final VoidCallback onSearchEscape;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final state = ref.watch(sftpControllerProvider);
    final controller = ref.read(sftpControllerProvider.notifier);
    final activity = ref.watch(sftpActivityProvider);
    final activeTransfers = (ref.watch(transfersProvider).value ?? const <TransferJob>[])
        .where((t) => t.isActive)
        .length;
    final edits = ref.watch(editSessionsProvider).value ?? const <EditSessionInfo>[];
    final attention = edits.any((e) => e.status.needsAttention);
    final host = state.host;

    Widget tool({
      required String key,
      required IconData icon,
      required String tooltip,
      required VoidCallback? onPressed,
      bool selected = false,
      int count = 0,
      bool alert = false,
    }) {
      final button = GlassIconButton(
        key: ValueKey(key),
        icon: icon,
        tooltip: tooltip,
        style: GlassIconButtonStyle.plain,
        selected: selected,
        onPressed: onPressed,
      );
      if (count == 0) return button;
      return Stack(
        clipBehavior: Clip.none,
        children: [
          button,
          PositionedDirectional(
            end: 0,
            top: 1,
            child: _CountBadge(count: count, alert: alert),
          ),
        ],
      );
    }

    final hostGroup = GlassToolbarGroup(
      padding: const EdgeInsetsDirectional.only(start: GlassSpacing.s12, end: GlassSpacing.s4),
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.dns_rounded, size: GlassSizes.iconRow, color: tokens.palette.accent),
            const SizedBox(width: GlassSpacing.s8),
            Flexible(
              child: Tooltip(
                message: host == null ? '' : '${host.name} · ${host.address}',
                child: Text(
                  host?.name ?? '',
                  key: const ValueKey('sftp-toolbar-title'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
              ),
            ),
            const SizedBox(width: GlassSpacing.s6),
            const GlassBadge(label: 'SFTP', tone: GlassTone.accent, dense: true),
            const SizedBox(width: GlassSpacing.s2),
            GlassIconButton(
              key: const ValueKey('sftp-toolbar-switch-host'),
              icon: Icons.unfold_more_rounded,
              tooltip: l10n.sftpToolbarSwitchHost,
              style: GlassIconButtonStyle.plain,
              iconSize: GlassSizes.iconRow,
              onPressed: onSwitchHost,
            ),
          ],
        ),
      ],
    );

    final actionsGroup = GlassToolbarGroup(
      children: [
        if (!AppPlatform.isMobile)
          tool(
            key: 'sftp-toolbar-local-pane',
            icon: Icons.vertical_split_rounded,
            tooltip: state.prefs.showLocalPane ? l10n.sftpToolbarHideLocal : l10n.sftpToolbarShowLocal,
            selected: state.prefs.showLocalPane,
            onPressed: controller.toggleLocalPane,
          ),
        tool(
          key: 'sftp-toolbar-quicklook',
          icon: Icons.visibility_rounded,
          tooltip: l10n.sftpToolbarQuickLook,
          onPressed: onQuickLook,
        ),
        GlassMenuButton<SftpAction>(
          entries: actionMenuEntries,
          onSelected: onAction,
          builder: (context, open) => tool(
            key: 'sftp-toolbar-actions',
            icon: Icons.more_horiz_rounded,
            tooltip: l10n.sftpToolbarActions,
            onPressed: open,
          ),
        ),
        tool(
          key: 'sftp-toolbar-refresh',
          icon: Icons.refresh_rounded,
          tooltip: l10n.sftpRefresh,
          onPressed: controller.refresh,
        ),
        tool(
          key: 'sftp-toolbar-transfers',
          icon: Icons.swap_vert_rounded,
          tooltip: l10n.sftpToolbarTransfers,
          selected: activity.open && activity.tab == SftpActivityTab.transfers,
          count: activeTransfers,
          onPressed: () => ref.read(sftpActivityProvider.notifier).toggle(SftpActivityTab.transfers),
        ),
        if (!AppPlatform.isMobile)
          tool(
            key: 'sftp-toolbar-editing',
            icon: Icons.edit_note_rounded,
            tooltip: l10n.sftpToolbarEditing,
            selected: activity.open && activity.tab == SftpActivityTab.editing,
            count: edits.length,
            alert: attention,
            onPressed: () => ref.read(sftpActivityProvider.notifier).toggle(SftpActivityTab.editing),
          ),
      ],
    );

    if (AppPlatform.isMobile) {
      return Padding(
        padding: const EdgeInsets.all(8),
        child: Column(
          children: [
            Row(
              children: [
                Expanded(child: hostGroup),
                GlassIconButton(
                  key: const ValueKey('sftp-disconnect'),
                  icon: Icons.link_off_rounded,
                  tooltip: l10n.sftpDisconnect,
                  onPressed: onDisconnect,
                ),
              ],
            ),
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(
                  child: _SearchField(
                    controller: searchController,
                    focusNode: searchFocus,
                    onChanged: controller.setFilter,
                    onEscape: onSearchEscape,
                  ),
                ),
                const SizedBox(width: 4),
                GlassMenuButton<SftpAction>(
                  entries: actionMenuEntries,
                  onSelected: onAction,
                  builder: (context, open) => tool(
                    key: 'sftp-toolbar-actions',
                    icon: Icons.more_horiz_rounded,
                    tooltip: l10n.sftpToolbarActions,
                    onPressed: open,
                  ),
                ),
                tool(
                  key: 'sftp-toolbar-refresh',
                  icon: Icons.refresh_rounded,
                  tooltip: l10n.sftpRefresh,
                  onPressed: controller.refresh,
                ),
              ],
            ),
          ],
        ),
      );
    }
    return SizedBox(
      height: GlassSizes.toolbarGroup,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final searchWidth = (constraints.maxWidth * 0.24).clamp(140.0, 240.0);
          return Row(
            children: [
              Flexible(
                child: Align(alignment: AlignmentDirectional.centerStart, child: hostGroup),
              ),
              const SizedBox(width: GlassSpacing.toolbarGroupGap),
              actionsGroup,
              const SizedBox(width: GlassSpacing.toolbarGroupGap),
              SizedBox(
                width: searchWidth,
                child: _SearchField(
                  controller: searchController,
                  focusNode: searchFocus,
                  onChanged: controller.setFilter,
                  onEscape: onSearchEscape,
                ),
              ),
              const SizedBox(width: GlassSpacing.toolbarGroupGap),
              GlassIconButton(
                key: const ValueKey('sftp-disconnect'),
                icon: Icons.link_off_rounded,
                tooltip: l10n.sftpDisconnect,
                onPressed: onDisconnect,
              ),
            ],
          );
        },
      ),
    );
  }
}

/// Count on a toolbar icon (active transfers, edit sessions): accent fill,
/// danger fill when a session needs attention.
class _CountBadge extends StatelessWidget {
  const _CountBadge({required this.count, required this.alert});

  final int count;
  final bool alert;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final caption = tokens.typography.caption;
    return IgnorePointer(
      child: DecoratedBox(
        decoration: ShapeDecoration(color: alert ? p.dangerFill : p.accentFill, shape: const StadiumBorder()),
        child: ConstrainedBox(
          constraints: const BoxConstraints(minWidth: 14, minHeight: 14),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 3),
            child: Center(
              widthFactor: 1,
              child: Text(
                '$count',
                maxLines: 1,
                style: caption.copyWith(
                  fontSize: caption.fontSize! - 2,
                  height: 1,
                  fontWeight: FontWeight.w700,
                  color: alert ? p.onDangerFill : p.onAccent,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _SearchField extends StatelessWidget {
  const _SearchField({
    required this.controller,
    required this.focusNode,
    required this.onChanged,
    required this.onEscape,
  });

  final TextEditingController controller;
  final FocusNode focusNode;
  final ValueChanged<String> onChanged;
  final VoidCallback onEscape;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): onEscape},
      child: ListenableBuilder(
        listenable: controller,
        builder: (context, _) => GlassField(
          key: const ValueKey('sftp-search'),
          controller: controller,
          focusNode: focusNode,
          search: true,
          leadingIcon: Icons.search_rounded,
          placeholder: l10n.sftpSearchHint,
          onChanged: onChanged,
          trailing: controller.text.isEmpty
              ? null
              : GlassIconButton(
                  icon: Icons.close_rounded,
                  tooltip: l10n.sftpSearchClear,
                  style: GlassIconButtonStyle.plain,
                  size: 22,
                  iconSize: 14,
                  onPressed: () {
                    controller.clear();
                    onChanged('');
                  },
                ),
        ),
      ),
    );
  }
}
