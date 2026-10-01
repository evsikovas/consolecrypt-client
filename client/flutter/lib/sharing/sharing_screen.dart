import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/enrollment_owner.dart';
import 'package:consolecrypt/sharing/enrollment_pairing.dart';
import 'package:consolecrypt/sharing/sharing_collections.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_management.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_secrets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class SharingScreen extends ConsumerStatefulWidget {
  const SharingScreen({super.key});
  @override
  ConsumerState<SharingScreen> createState() => _SharingScreenState();
}

class _SharingScreenState extends ConsumerState<SharingScreen> {
  bool _owned = false, _busy = false;
  Future<void> _refresh() async {
    if (_busy) return;
    final scope = ref.read(sharingSessionScopeProvider);
    final service = ref.read(sharingServiceProvider);
    setState(() => _busy = true);
    await runWithFeedback(context, () async {
      // Recheck support even after an unavailable/error result. The retained
      // shell page must not require signing out after a server flag changes.
      final status = await ref.refresh(sharingStatusProvider.future);
      if (!mounted || !identical(scope, ref.read(sharingSessionScopeProvider))) return;
      if (status.enabled && !status.locked) await service.list(refresh: true);
    });
    if (mounted) {
      if (identical(scope, ref.read(sharingSessionScopeProvider))) {
        ref.invalidate(sharingItemsProvider);
        ref.invalidate(sharingOutboxProvider);
      }
      setState(() => _busy = false);
    }
  }

  Future<void> _flush() async {
    if (_busy) return;
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
    if (mounted) {
      refreshSharing(ref);
      setState(() => _busy = false);
    }
  }

  Future<void> _identity(SharingIdentity identity) {
    final scope = ref.read(sharingSessionScopeProvider);
    return showAppDialog<void>(
      context,
      secure: true,
      builder: (context) => SharingDialogGuard(
        profile: scope,
        child: GlassDialog(
          title: context.l10n.sharingIdentity,
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(context.l10n.sharingVerifyHelp),
              const SizedBox(height: 12),
              Text('${context.l10n.sharingDevice}: ${identity.deviceId}'),
              SelectableText(identity.code, style: const TextStyle(fontFamily: 'monospace')),
            ],
          ),
          primaryAction: GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context)),
          secondaryActions: [
            GlassButton(
              label: context.l10n.sharingCopyCode,
              onPressed: () => copyPlainWithNotice(context, ref, identity.code),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final status = ref.watch(sharingStatusProvider);
    final items = ref.watch(sharingItemsProvider);
    final local = ref.watch(activeProfileProvider)?.isLocal ?? true;
    final unlocked = ref.watch(vaultStatusProvider).value?.phase == VaultPhase.unlocked;
    return PageScaffold(
      title: l.sharingTitle,
      subtitle: l.sharingSubtitle,
      actions: [
        if (unlocked && status.value?.identity != null)
          GlassButton(
            icon: Icons.fingerprint,
            label: l.sharingIdentity,
            onPressed: () => _identity(status.value!.identity!),
          ),
        GlassIconButton(
          icon: Icons.refresh_rounded,
          tooltip: l.sharingRefresh,
          onPressed: _busy || local || !unlocked || status.isLoading ? null : _refresh,
        ),
      ],
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (status.isLoading || _busy) const LinearProgressIndicator(),
          if (local || status.hasError || status.value?.enabled == false)
            SectionCard(
              title: l.sharingTitle,
              icon: Icons.info_outline,
              child: Text(
                local
                    ? l.sharingLocal
                    : status.hasError &&
                          !(status.error is AppException &&
                              [
                                AppErrorCode.unsupported,
                                AppErrorCode.notFound,
                              ].contains((status.error as AppException).code))
                    ? errorMessage(l, status.error!)
                    : l.sharingUnavailable,
              ),
            ),
          if (unlocked && status.value?.enabled == true) ...[
            if (status.value?.supportsOwnerOnlineEnrollment == true)
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  GlassButton(
                    label: l.enrollmentPrepare,
                    icon: Icons.add_to_home_screen_rounded,
                    onPressed: () => showEnrollmentPairing(context, EnrollmentPairingFlow.prepare),
                  ),
                  GlassButton(
                    label: l.enrollmentEndorse,
                    icon: Icons.verified_user_outlined,
                    onPressed: () => showEnrollmentPairing(context, EnrollmentPairingFlow.endorse),
                  ),
                  GlassButton(
                    label: l.enrollmentSubmit,
                    icon: Icons.devices_outlined,
                    onPressed: () => showEnrollmentPairing(context, EnrollmentPairingFlow.submit),
                  ),
                  GlassButton(
                    label: l.sharingReconcile,
                    icon: Icons.history_rounded,
                    onPressed: () => showEnrollmentPairing(context, EnrollmentPairingFlow.restore),
                  ),
                  GlassButton(
                    label: l.enrollmentRequests,
                    icon: Icons.pending_actions_rounded,
                    onPressed: () => showEnrollmentPending(context),
                  ),
                ],
              ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                ChoiceChip(
                  key: const ValueKey('sharing-incoming'),
                  label: Text(l.sharingReceived),
                  selected: !_owned,
                  onSelected: (_) => setState(() => _owned = false),
                ),
                ChoiceChip(
                  key: const ValueKey('sharing-owned'),
                  label: Text(l.sharingOwned),
                  selected: _owned,
                  onSelected: (_) => setState(() => _owned = true),
                ),
              ],
            ),
            if ((status.value?.pending ?? 0) + (status.value?.blocked ?? 0) > 0) ...[
              const SizedBox(height: 12),
              SectionCard(
                title: l.sharingQueue,
                icon: Icons.cloud_upload_outlined,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      '${l.sharingPending}: ${status.value!.pending} · ${l.sharingBlocked}: ${status.value!.blocked}',
                    ),
                    if (status.value!.blocked > 0) Text(l.sharingConflictHelp),
                    Wrap(
                      spacing: 8,
                      runSpacing: 8,
                      children: [
                        GlassButton(label: l.sharingFlush, onPressed: _busy ? null : _flush),
                        GlassButton(label: l.sharingReviewQueue, onPressed: () => showSharingQueue(context)),
                      ],
                    ),
                  ],
                ),
              ),
            ],
            const SizedBox(height: 12),
            Expanded(
              child: items.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(child: Text(l.sharingBlocked)),
                data: (all) {
                  final shown = all.where((i) => i.owned == _owned && i.trust != SharingTrust.deleted).toList();
                  if (shown.isEmpty) return Center(child: Text(l.sharingEmpty));
                  return ListView.separated(
                    itemCount: shown.length,
                    separatorBuilder: (_, _) => const SizedBox(height: 12),
                    itemBuilder: (_, index) => _SharingTile(shown[index], status.value!),
                  );
                },
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _SharingTile extends ConsumerWidget {
  const _SharingTile(this.item, this.status, {this.visited = const {}});
  final SharingItem item;
  final SharingStatus status;
  final Set<String> visited;
  bool get manages => item.owned && item.ownerDeviceId == status.identity?.deviceId;
  Future<void> _copy(BuildContext context, WidgetRef ref) async {
    if (item.kind == SharingKind.host) {
      await showSharingCopyHost(context, item);
      return;
    }
    final profile = ref.read(sharingSessionScopeProvider);
    if (item.previewJson == null || !sharingSessionCurrent(ref, profile)) return;
    final ok = await showConfirmDialog(
      context,
      title: context.l10n.sharingCopyPersonal,
      message: context.l10n.sharingCopyHelp,
      confirmLabel: context.l10n.commonSave,
    );
    if (!ok || !context.mounted || !sharingSessionCurrent(ref, profile)) return;
    await runWithFeedback(
      context,
      () => ref.read(sharingServiceProvider).copySnippet(item.id),
      success: context.l10n.sharingSaved,
    );
  }

  Future<void> _revoke(BuildContext context, WidgetRef ref, SharingGrant removed) async {
    final profile = ref.read(sharingSessionScopeProvider);
    final ok = await showConfirmDialog(
      context,
      title: context.l10n.sharingRevoke,
      message: context.l10n.sharingRevokeHelp,
      confirmLabel: context.l10n.sharingRevoke,
      destructive: true,
    );
    if (!ok || !context.mounted || !sharingSessionCurrent(ref, profile)) return;
    await runWithFeedback(context, () async {
      await ref
          .read(sharingServiceProvider)
          .rotate(
            item.id,
            item.memberRoles
                .where(
                  (g) => g.identity.deviceId != removed.identity.deviceId && g.identity.deviceId != item.ownerDeviceId,
                )
                .toList(),
          );
      await ref.read(sharingServiceProvider).flush();
      return true;
    });
    if (context.mounted) refreshSharing(ref);
  }

  Future<void> _delete(BuildContext context, WidgetRef ref) async {
    final profile = ref.read(sharingSessionScopeProvider);
    final ok = await showConfirmDialog(
      context,
      title: context.l10n.sharingStop,
      message: context.l10n.sharingStopHelp,
      confirmLabel: context.l10n.sharingStop,
      destructive: true,
    );
    if (!ok || !context.mounted || !sharingSessionCurrent(ref, profile)) return;
    await runWithFeedback(context, () async {
      await ref.read(sharingServiceProvider).delete(item.id);
      await ref.read(sharingServiceProvider).flush();
      return true;
    });
    if (context.mounted) refreshSharing(ref);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final pending = (ref.watch(sharingOutboxProvider).value ?? const <SharingOutboxEntry>[]).any(
      (entry) => entry.shareId == item.id,
    );
    final label = switch (item.kind) {
      SharingKind.host => l.sharingHost,
      SharingKind.snippet => l.sharingSnippet,
      SharingKind.group => l.sharingGroup,
      SharingKind.secret => l.sharingSecret,
    };
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: Icon(switch (item.kind) {
              SharingKind.host => Icons.dns_outlined,
              SharingKind.snippet => Icons.code_rounded,
              SharingKind.group => Icons.folder_shared_outlined,
              SharingKind.secret => Icons.key_outlined,
            }),
            title: Text(item.name ?? l.sharingInvitation),
            subtitle: Text(
              '$label · ${item.role == SharingRole.editor ? l.sharingEditor : l.sharingReader} · ${l.sharingRevision} ${item.revision}',
            ),
            trailing: Icon(item.trust == SharingTrust.verified ? Icons.verified_user_outlined : Icons.shield_outlined),
          ),
          if (item.trust == SharingTrust.unverified)
            GlassButton.prominent(label: l.sharingAccept, onPressed: () => showSharingAccept(context, item)),
          if (item.trust == SharingTrust.blocked) ...[
            Text(
              item.blockedReason == 'sharing_reconciliation_required' ? l.sharingReconcileHelp : l.sharingConflictHelp,
            ),
            if (item.blockedReason == 'sharing_reconciliation_required')
              GlassButton(
                label: l.sharingReconcile,
                onPressed: () => showAppDialog<void>(
                  context,
                  secure: true,
                  builder: (_) => SharingAcceptDialog(item, reconcile: true),
                ),
              ),
          ],
          if (item.previewJson != null && item.trust == SharingTrust.verified) ...[
            SharingProjectionView(item.previewJson!),
            if (item.kind == SharingKind.group)
              SharingCollectionView(
                group: item,
                items: ref.watch(sharingItemsProvider).value ?? const [],
                onOpen: (child) {
                  if (visited.contains(child.id) || child.id == item.id || visited.length >= 8) return;
                  final profile = ref.read(sharingSessionScopeProvider);
                  showAppDialog<void>(
                    context,
                    secure: true,
                    builder: (context) => SharingDialogGuard(
                      profile: profile,
                      child: GlassDialog(
                        title: child.name ?? l.sharingInvitation,
                        content: SizedBox(
                          width: 600,
                          child: ConstrainedBox(
                            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
                            child: SingleChildScrollView(
                              child: _SharingTile(child, status, visited: {...visited, item.id}),
                            ),
                          ),
                        ),
                        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
                      ),
                    ),
                  );
                },
              ),
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                if (item.kind == SharingKind.host || item.kind == SharingKind.snippet)
                  GlassButton(
                    label: l.sharingCopyPersonal,
                    icon: Icons.copy_outlined,
                    onPressed: () => _copy(context, ref),
                  ),
                if (item.kind == SharingKind.secret)
                  GlassButton(
                    label: l.sharingSecret,
                    icon: Icons.key_outlined,
                    onPressed: () => showSharingSecret(context, item),
                  ),
                if (!pending && item.canEdit && item.kind == SharingKind.group)
                  GlassButton(
                    label: l.sharingEdit,
                    icon: Icons.edit_outlined,
                    onPressed: () => showSharingGroupEdit(context, item),
                  ),
                if (!pending && item.canEdit && (item.kind == SharingKind.host || item.kind == SharingKind.snippet))
                  GlassButton(
                    label: l.sharingEdit,
                    icon: Icons.edit_outlined,
                    onPressed: () =>
                        showAppDialog<void>(context, secure: true, builder: (_) => SharingEditDialog(item)),
                  ),
                if (manages && !pending)
                  GlassButton(label: l.sharingStop, icon: Icons.delete_outline, onPressed: () => _delete(context, ref)),
              ],
            ),
            if (manages) ...[
              const SizedBox(height: 12),
              GlassButton(
                label: l.sharingManageAccess,
                icon: Icons.manage_accounts_outlined,
                onPressed: pending ? null : () => showSharingAccess(context, item),
              ),
              GlassButton(
                label: l.enrollmentTitle,
                icon: Icons.devices_outlined,
                onPressed: pending ? null : () => showEnrollmentOwner(context, item),
              ),
              Text(l.sharingMembers),
              for (final member in item.memberRoles)
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  title: Text(member.identity.deviceId),
                  subtitle: Text(member.role == SharingRole.editor ? l.sharingEditor : l.sharingReader),
                  trailing: member.identity.deviceId == item.ownerDeviceId
                      ? null
                      : GlassIconButton(
                          icon: Icons.person_remove_outlined,
                          tooltip: l.sharingRevoke,
                          onPressed: pending ? null : () => _revoke(context, ref, member),
                        ),
                ),
            ],
            if (item.owned && !manages) Text(l.sharingOwnerDeviceOnly),
          ],
        ],
      ),
    );
  }
}

class SharingEditDialog extends ConsumerStatefulWidget {
  const SharingEditDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingEditDialog> createState() => _SharingEditDialogState();
}

class _SharingEditDialogState extends ConsumerState<SharingEditDialog> {
  final Map<String, TextEditingController> _fields = {};
  late final Object? _profile;
  bool _busy = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    final d = widget.item.data!;
    for (final key
        in widget.item.kind == SharingKind.host
            ? ['name', 'address', 'port', 'username', 'notes']
            : ['name', 'description', 'template']) {
      _fields[key] = TextEditingController(text: d[key]?.toString() ?? '');
    }
  }

  @override
  void dispose() {
    for (final c in _fields.values) {
      c.clear();
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (!sharingSessionCurrent(ref, _profile)) return;
    final p = widget.item.projection!;
    final d = p['data'] as Map<String, dynamic>;
    for (final field in _fields.entries) {
      d[field.key] = field.key == 'port' ? int.tryParse(field.value.text) : field.value.text;
    }
    setState(() => _busy = true);
    final result = await runWithFeedback(
      context,
      () => ref.read(sharingServiceProvider).edit(widget.item.id, jsonEncode(p)),
      success: context.l10n.sharingChangeSaved,
    );
    if (!mounted) return;
    if (result != null && sharingSessionCurrent(ref, _profile)) {
      await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      refreshSharing(ref);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final labels = {
      'name': l.sharingName,
      'address': l.sharingAddress,
      'port': l.sharingPort,
      'username': l.sharingUsername,
      'notes': l.sharingNotes,
      'description': l.sharingDescription,
      'template': l.sharingCommand,
    };
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        title: l.sharingEdit,
        content: SizedBox(
          width: 560,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final field in _fields.entries)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 12),
                      child: TextField(
                        controller: field.value,
                        decoration: InputDecoration(labelText: labels[field.key]),
                        maxLines: field.key == 'template' || field.key == 'notes' ? 5 : 1,
                        keyboardType: field.key == 'port' ? TextInputType.number : TextInputType.text,
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(label: l.commonSave, onPressed: _busy ? null : _save),
      ),
    );
  }
}
