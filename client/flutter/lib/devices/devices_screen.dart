import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/devices/approve_device_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

IconData platformIcon(DevicePlatform p) => switch (p) {
  DevicePlatform.macos => Icons.laptop_mac_rounded,
  DevicePlatform.windows => Icons.laptop_windows_rounded,
  DevicePlatform.linux => Icons.computer_rounded,
  DevicePlatform.ios || DevicePlatform.android => Icons.smartphone_rounded,
  DevicePlatform.cli => Icons.terminal_rounded,
};

class DevicesScreen extends ConsumerWidget {
  const DevicesScreen({super.key});

  Future<void> _revoke(BuildContext context, WidgetRef ref, DeviceInfo d) async {
    final l10n = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l10n.devicesRevokeTitle(d.name),
      message: l10n.devicesRevokeMessage,
      confirmLabel: l10n.devicesRevokeConfirm,
      destructive: true,
    );
    if (ok && context.mounted) {
      await runWithFeedback(
        context,
        () => ref.read(devicesServiceProvider).revoke(d.deviceId),
        success: l10n.devicesRevoked(d.name),
      );
    }
  }

  Future<void> _rename(BuildContext context, WidgetRef ref, DeviceInfo d) async {
    final name = await showTextInputDialog(context, title: context.l10n.devicesRenameTitle, initial: d.name);
    if (name != null && context.mounted) {
      await runWithFeedback(context, () => ref.read(devicesServiceProvider).rename(d.deviceId, name));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profile = ref.watch(activeProfileProvider);
    final l10n = context.l10n;
    if (profile != null && profile.isLocal) {
      return PageScaffold(
        title: l10n.devicesTitle,
        body: EmptyState(
          icon: Icons.devices_rounded,
          title: l10n.devicesLocalTitle,
          message: l10n.devicesLocalMessage,
          action: GlassButton.prominent(
            size: GlassControlSize.lg,
            onPressed: () => context.go(AppRoutes.settings),
            label: l10n.devicesEnableSync,
          ),
        ),
      );
    }
    final snapshotAsync = ref.watch(devicesProvider);
    final vaultId = ref.watch(vaultStatusProvider).value?.vaultId;
    final email = profile?.accountEmail;
    return PageScaffold(
      title: l10n.devicesTitle,
      subtitle: email == null ? l10n.devicesSubtitleNoEmail : l10n.devicesSubtitle(email),
      actions: [
        GlassIconButton(
          key: const ValueKey('devices-refresh'),
          tooltip: l10n.devicesRefresh,
          icon: Icons.refresh_rounded,
          onPressed: () => ref.read(devicesServiceProvider).refresh(),
        ),
      ],
      body: AsyncValueView(
        value: snapshotAsync,
        data: (snapshot) => ListView(
          children: [
            for (final r in snapshot.pendingRequests) ...[
              _PendingRequestCard(request: r),
              const SizedBox(height: GlassSpacing.s12),
            ],
            ContentSurface(
              padding: const EdgeInsets.all(GlassSpacing.s4),
              child: Column(
                children: [
                  for (final (i, d) in snapshot.devices.indexed) ...[
                    if (i > 0)
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
                        child: Divider(height: 1, color: GlassTokens.of(context).surfaces.separator),
                      ),
                    _DeviceTile(
                      device: d,
                      vaultId: vaultId,
                      pending: snapshot.pendingRequests.any((r) => r.device.deviceId == d.deviceId),
                      onRevoke: () => _revoke(context, ref, d),
                      onRename: () => _rename(context, ref, d),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _PendingRequestCard extends ConsumerWidget {
  const _PendingRequestCard({required this.request});

  final DeviceTrustRequest request;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final d = request.device;
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final t = tokens.typography;
    final ago = formatRelative(l10n, request.createdAt);
    final platform = d.platform.localized(l10n);
    // A content-layer banner card in the warning role (§4.1), not glass.
    return DecoratedBox(
      key: ValueKey('pending-${d.name}'),
      decoration: ShapeDecoration(
        color: Color.alphaBlend(p.warningFill.withValues(alpha: 0.12), tokens.surfaces.content),
        shape: GlassRadii.shape(tokens.radii.card).copyWith(side: BorderSide(color: p.warning.withValues(alpha: 0.25))),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.card),
        child: Row(
          children: [
            Icon(platformIcon(d.platform), size: 32, color: p.warning),
            const SizedBox(width: GlassSpacing.s16),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(l10n.devicesPendingTitle(d.name), style: t.title3.copyWith(color: p.label)),
                  const SizedBox(height: GlassSpacing.s2),
                  Text(
                    request.expiresAt.isAfter(DateTime.now())
                        ? l10n.devicesPendingMeta(platform, ago, formatRemaining(l10n, request.expiresAt))
                        : l10n.devicesPendingMetaExpired(platform, ago),
                    style: t.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                  const SizedBox(height: GlassSpacing.s4),
                  Text(l10n.devicesPendingHint),
                ],
              ),
            ),
            const SizedBox(width: GlassSpacing.s12),
            GlassButton(
              style: GlassButtonStyle.destructiveQuiet,
              onPressed: () => runWithFeedback(
                context,
                () => ref.read(devicesServiceProvider).reject(request.requestId),
                success: l10n.devicesRequestRejected,
              ),
              label: l10n.devicesReject,
            ),
            const SizedBox(width: GlassSpacing.s8),
            GlassButton(
              key: const ValueKey('review-request'),
              icon: Icons.verified_user_rounded,
              onPressed: () => showApproveDeviceDialog(context, request),
              label: l10n.devicesReviewApprove,
            ),
          ],
        ),
      ),
    );
  }
}

enum _DeviceAction { rename, revoke }

class _DeviceTile extends StatelessWidget {
  const _DeviceTile({
    required this.device,
    required this.vaultId,
    required this.pending,
    required this.onRevoke,
    required this.onRename,
  });

  final DeviceInfo device;
  final VaultId? vaultId;
  final bool pending;
  final VoidCallback onRevoke;
  final VoidCallback onRename;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final l10n = context.l10n;
    final (label, tone, icon) = device.isRevoked
        ? (l10n.devicesStatusRevoked, GlassTone.danger, Icons.block_rounded)
        : device.isTrustedFor(vaultId)
        ? (l10n.devicesStatusTrusted, GlassTone.success, Icons.verified_rounded)
        : (
            pending ? l10n.devicesStatusAwaitingApproval : l10n.devicesStatusNotTrusted,
            GlassTone.warning,
            Icons.hourglass_top_rounded,
          );
    return ListTile(
      key: ValueKey('device-${device.name}'),
      leading: Icon(platformIcon(device.platform)),
      title: Row(
        children: [
          Flexible(
            child: Text(
              device.name,
              overflow: TextOverflow.ellipsis,
              style: tokens.typography.bodyEmph.copyWith(color: p.label),
            ),
          ),
          if (device.isCurrent) ...[
            const SizedBox(width: GlassSpacing.s8),
            GlassBadge(label: l10n.devicesThisDevice, tone: GlassTone.accent, dense: true),
          ],
        ],
      ),
      subtitle: Text(
        [
          device.platform.localized(l10n),
          if (device.lastSeenAt != null) l10n.devicesLastSeen(formatRelative(l10n, device.lastSeenAt!)),
          l10n.devicesAdded(formatDate(l10n, device.createdAt)),
          if (device.revokedAt != null) l10n.devicesRevokedOn(formatDate(l10n, device.revokedAt!)),
        ].join(' · '),
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          GlassBadge(label: label, tone: tone, icon: icon),
          const SizedBox(width: GlassSpacing.s4),
          GlassMenuButton<_DeviceAction>(
            entries: [
              GlassMenuItem(value: _DeviceAction.rename, label: l10n.commonRename, icon: Icons.edit_rounded),
              if (!device.isCurrent)
                GlassMenuItem(
                  key: ValueKey('revoke-${device.name}'),
                  value: _DeviceAction.revoke,
                  label: l10n.devicesRevokeMenu,
                  icon: Icons.block_rounded,
                  destructive: true,
                ),
            ],
            onSelected: (v) => v == _DeviceAction.rename ? onRename() : onRevoke(),
            builder: (context, open) => GlassIconButton(
              key: ValueKey('device-menu-${device.name}'),
              icon: Icons.more_horiz_rounded,
              tooltip: GlassStrings.of(context).moreActions,
              style: GlassIconButtonStyle.plain,
              onPressed: device.isRevoked ? null : open,
            ),
          ),
        ],
      ),
    );
  }
}
