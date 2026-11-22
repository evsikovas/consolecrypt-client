import 'package:consolecrypt/account/enterprise_sign_in.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/workspace_sections.dart';
import 'package:consolecrypt/devices/approve_device_dialog.dart';
import 'package:consolecrypt/vault/vault_key_trust.dart';
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

class DevicesScreen extends ConsumerStatefulWidget {
  const DevicesScreen({super.key});
  @override
  ConsumerState<DevicesScreen> createState() => _DevicesState();
}

class _DevicesState extends ConsumerState<DevicesScreen> {
  DeviceRequestId? selectedRequest;

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
  Widget build(BuildContext context) {
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
    final hints = ref.watch(signInHintsProvider).value ?? ref.read(authServiceProvider).currentSignInHints;
    final signIn = profileSignIn(hints, profile);
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
        data: (snapshot) {
          final requests = snapshot.pendingRequests;
          final selected = requests.where((r) => r.requestId == selectedRequest).firstOrNull ?? requests.firstOrNull;
          final trusted = snapshot.devices.where((d) => !d.isRevoked && d.isTrustedFor(vaultId)).toList();
          final remaining = snapshot.devices.where((d) => !trusted.contains(d)).toList();
          Widget table(String title, List<DeviceInfo> devices) => WorkspaceSection(
            title: title,
            trailing: Text(devices.length.toString()),
            child: LayoutBuilder(
              builder: (context, bounds) => Column(
                children: [
                  if (bounds.maxWidth >= 640)
                    WorkspaceColumns(
                      actionWidth: 34,
                      cells: [Text(l10n.uiDevice), Text(l10n.uiPlatform), Text(l10n.uiLastActive), Text(l10n.uiTrust)],
                    ),
                  for (final d in devices) ...[
                    const Divider(height: 1),
                    _DeviceTile(
                      device: d,
                      vaultId: vaultId,
                      protection: snapshot.deviceProtection,
                      pending: requests.any((r) => r.device.deviceId == d.deviceId),
                      onRevoke: () => _revoke(context, ref, d),
                      onRename: () => _rename(context, ref, d),
                    ),
                  ],
                ],
              ),
            ),
          );
          final inventory = Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 12,
            children: [
              table(l10n.uiTrustedDevices, trusted),
              if (remaining.isNotEmpty) table(l10n.uiOtherDevices, remaining),
            ],
          );
          return ListView(
            children: [
              if (signIn != null && signIn.method.isCorporate) ...[
                InfoBanner(
                  key: const ValueKey('devices-auth-notice'),
                  icon: Icons.apartment_rounded,
                  message: l10n.devicesAuthCorporate(
                    corporateAuthName(l10n, displayName: signIn.displayName, method: signIn.method),
                  ),
                ),
                const SizedBox(height: GlassSpacing.s12),
              ],
              const VaultKeyUnconfirmedBanner(),
              VaultKeyConflictBanners(snapshot: snapshot, onRevoke: (d) => _revoke(context, ref, d)),
              if (selected == null)
                inventory
              else
                WorkspaceColumnsLayout(
                  main: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    spacing: 12,
                    children: [
                      inventory,
                      WorkspaceSection(
                        title: l10n.uiPendingRequests,
                        trailing: Text(requests.length.toString()),
                        child: Column(
                          children: [
                            for (final r in requests)
                              ListTile(
                                dense: true,
                                selected: r.requestId == selected.requestId,
                                selectedTileColor: GlassTokens.of(context).sidebarSelection,
                                title: Text(r.device.name),
                                subtitle: Text(r.device.platform.localized(l10n)),
                                leading: Icon(platformIcon(r.device.platform)),
                                trailing: const Icon(Icons.chevron_right),
                                onTap: () => setState(() => selectedRequest = r.requestId),
                              ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  side: _PendingRequestCard(key: ValueKey(selected.requestId), request: selected),
                ),
            ],
          );
        },
      ),
    );
  }
}

class _PendingRequestCard extends ConsumerWidget {
  const _PendingRequestCard({super.key, required this.request});

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
    final details = Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      spacing: GlassSpacing.s16,
      children: [
        Icon(platformIcon(d.platform), size: 32, color: p.warning),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: GlassSpacing.s4,
            children: [
              Text(l10n.devicesPendingTitle(d.name), style: t.title3.copyWith(color: p.label)),
              Text(
                request.expiresAt.isAfter(DateTime.now())
                    ? l10n.devicesPendingMeta(platform, ago, formatRemaining(l10n, request.expiresAt))
                    : l10n.devicesPendingMetaExpired(platform, ago),
                style: t.callout.copyWith(color: tokens.secondaryLabel),
              ),
              Text(l10n.devicesPendingHint),
              if (ref.watch(activeProfileProvider) case final profile?)
                Text([l10n.uiProfile, profile.name].join(' · '), style: t.caption),
            ],
          ),
        ),
      ],
    );
    final actions = Wrap(
      alignment: WrapAlignment.end,
      spacing: GlassSpacing.toolbarItemGap,
      runSpacing: GlassSpacing.s8,
      children: [
        GlassButton(
          style: GlassButtonStyle.destructiveQuiet,
          onPressed: () => runWithFeedback(
            context,
            () => ref.read(devicesServiceProvider).reject(request.requestId),
            success: l10n.devicesRequestRejected,
          ),
          label: l10n.devicesReject,
        ),
        GlassButton(
          key: const ValueKey('review-request'),
          icon: Icons.verified_user_rounded,
          onPressed: () => showApproveDeviceDialog(context, request),
          label: l10n.devicesReviewApprove,
        ),
      ],
    );
    // A content-layer banner card in the warning role (§4.1), not glass.
    return DecoratedBox(
      key: ValueKey('pending-${d.name}'),
      decoration: ShapeDecoration(
        color: Color.alphaBlend(p.warningFill.withValues(alpha: 0.12), tokens.surfaces.content),
        shape: GlassRadii.shape(tokens.radii.card).copyWith(side: BorderSide(color: p.warning.withValues(alpha: 0.25))),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.card),
        child: LayoutBuilder(
          builder: (context, constraints) => constraints.maxWidth < 640
              ? Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  spacing: GlassSpacing.s12,
                  children: [details, actions],
                )
              : Row(
                  spacing: GlassSpacing.s12,
                  children: [
                    Expanded(child: details),
                    Flexible(child: actions),
                  ],
                ),
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
    this.protection = false,
    required this.onRevoke,
    required this.onRename,
  });

  final DeviceInfo device;
  final VaultId? vaultId;
  final bool pending;

  /// The server reports account trust and quarantine (protocol 1.7).
  final bool protection;
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
            pending
                ? l10n.devicesStatusAwaitingApproval
                : protection && device.trusted == false
                ? l10n.devicesStatusUnconfirmed
                : l10n.devicesStatusNotTrusted,
            GlassTone.warning,
            Icons.hourglass_top_rounded,
          );
    final quarantine = protection && device.inQuarantine ? device.quarantineUntil : null;
    final metadata = [
      device.platform.localized(l10n),
      if (device.lastSeenAt != null) l10n.devicesLastSeen(formatRelative(l10n, device.lastSeenAt!)),
      l10n.devicesAdded(formatDate(l10n, device.createdAt)),
      if (device.revokedAt != null) l10n.devicesRevokedOn(formatDate(l10n, device.revokedAt!)),
    ].join(' · ');
    final name = Text(
      device.name,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: tokens.typography.bodyEmph.copyWith(color: p.label),
    );
    final quarantineLabel = quarantine == null ? null : l10n.devicesQuarantineBadge(formatDate(l10n, quarantine));
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 640;
        final menu = GlassMenuButton<_DeviceAction>(
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
        );
        if (!compact) {
          return DecoratedBox(
            key: ValueKey('device-${device.name}'),
            decoration: BoxDecoration(color: device.isCurrent ? tokens.sidebarSelection : null),
            child: WorkspaceColumns(
              actionWidth: 34,
              actions: menu,
              cells: [
                Row(
                  children: [
                    Icon(platformIcon(device.platform), size: 16),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          name,
                          if (device.isCurrent) Text(l10n.devicesThisDevice, style: tokens.typography.caption),
                        ],
                      ),
                    ),
                  ],
                ),
                Text(device.platform.localized(l10n)),
                Tooltip(
                  message: metadata,
                  child: Text(device.lastSeenAt == null ? '—' : formatDateTime(l10n, device.lastSeenAt!)),
                ),
                Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(label, style: TextStyle(color: p.tone(tone))),
                    if (quarantineLabel != null) Text(quarantineLabel, key: ValueKey('quarantine-${device.name}')),
                  ],
                ),
              ],
            ),
          );
        }
        return ListTile(
          key: ValueKey('device-${device.name}'),
          leading: Icon(platformIcon(device.platform)),
          title: name,
          subtitle: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: GlassSpacing.s4,
            children: [
              Text(metadata),
              Row(
                spacing: GlassSpacing.s4,
                children: [
                  Icon(icon, size: 14, color: p.tone(tone)),
                  Flexible(child: Text(label)),
                ],
              ),
              if (device.isCurrent) Text(l10n.devicesThisDevice),
              if (quarantineLabel != null) Text(quarantineLabel, key: ValueKey('quarantine-${device.name}')),
            ],
          ),
          trailing: menu,
        );
      },
    );
  }
}
