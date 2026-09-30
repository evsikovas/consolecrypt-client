import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showSharingAccess(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingAccessDialog(item));

class SharingAccessDialog extends ConsumerStatefulWidget {
  const SharingAccessDialog(this.item, {super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingAccessDialog> createState() => _SharingAccessDialogState();
}

class _SharingAccessDialogState extends ConsumerState<SharingAccessDialog> {
  late final Object? _profile;
  late List<SharingGrant> _grants;
  bool _busy = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _grants = widget.item.memberRoles.where((g) => g.identity.deviceId != widget.item.ownerDeviceId).toList();
  }

  Future<void> _save() async {
    if (!sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    final queued = await runWithFeedback(
      context,
      () => ref.read(sharingServiceProvider).rotate(widget.item.id, _grants),
    );
    if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
    if (queued != null) {
      await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      refreshSharing(ref);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => SharingDialogGuard(
    profile: _profile,
    child: GlassDialog(
      title: context.l10n.sharingManageAccess,
      content: SizedBox(
        width: 600,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(context.l10n.sharingRevokeHelp),
                if (_busy) const LinearProgressIndicator(),
                AbsorbPointer(
                  absorbing: _busy,
                  child: SharingRecipients(
                    initialGrants: _grants,
                    excludedDeviceIds: {widget.item.ownerDeviceId},
                    onChanged: (g) => setState(() => _grants = g),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
      secondaryActions: [
        GlassButton(label: context.l10n.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('sharing-access-save'),
        label: context.l10n.commonSave,
        onPressed: _busy ? null : _save,
      ),
    ),
  );
}

Future<void> showSharingCopyHost(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => _SharingCopyHost(item));

class _SharingCopyHost extends ConsumerStatefulWidget {
  const _SharingCopyHost(this.item);
  final SharingItem item;
  @override
  ConsumerState<_SharingCopyHost> createState() => _SharingCopyHostState();
}

class _SharingCopyHostState extends ConsumerState<_SharingCopyHost> {
  late final Object? _profile;
  String? _credential;
  bool _busy = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
  }

  Future<void> _save() async {
    if (!sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    final saved = await runWithFeedback(
      context,
      () => ref.read(sharingServiceProvider).copyHost(widget.item.id, credentialId: _credential),
      success: context.l10n.sharingSaved,
    );
    if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
    if (saved != null) {
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final hosts = ref.watch(hostsProvider).value ?? const <Host>[];
    final inline = hosts.map((h) => h.inlineCredentialId).whereType<ObjectId>().toSet();
    final credentials = (ref.watch(credentialsProvider).value ?? const <Credential>[]).where(
      (c) => !inline.contains(c.id),
    );
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        title: l.sharingCopyPersonal,
        content: SizedBox(
          width: 500,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.sharingCopyHelp),
              const SizedBox(height: 12),
              GlassSelect<String>(
                value: _credential ?? '',
                items: [
                  GlassSelectItem(value: '', label: l.sharingPromptPassword),
                  for (final c in credentials) GlassSelectItem(value: c.id.value, label: c.name),
                ],
                onChanged: _busy ? null : (v) => setState(() => _credential = v.isEmpty ? null : v),
              ),
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-copy-host-save'),
          label: l.commonSave,
          onPressed: _busy ? null : _save,
        ),
      ),
    );
  }
}

Future<void> showSharingQueue(BuildContext context) =>
    showAppDialog<void>(context, secure: true, builder: (_) => const SharingQueueDialog());

class SharingQueueDialog extends ConsumerStatefulWidget {
  const SharingQueueDialog({super.key});
  @override
  ConsumerState<SharingQueueDialog> createState() => _SharingQueueDialogState();
}

class _SharingQueueDialogState extends ConsumerState<SharingQueueDialog> {
  late final Object? profile;
  @override
  void initState() {
    super.initState();
    profile = ref.read(sharingSessionScopeProvider);
  }

  @override
  Widget build(BuildContext context) {
    final entries = ref.watch(sharingOutboxProvider);
    final names = {for (final i in ref.watch(sharingItemsProvider).value ?? const <SharingItem>[]) i.id: i.name};
    final l = context.l10n;
    return SharingDialogGuard(
      profile: profile,
      child: GlassDialog(
        title: l.sharingQueue,
        content: SizedBox(
          width: 600,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: entries.when(
              loading: () => const LinearProgressIndicator(),
              error: (_, _) => Text(l.sharingUnavailable),
              data: (all) => all.isEmpty
                  ? Text(l.sharingEmpty)
                  : SingleChildScrollView(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(l.sharingQueueHelp),
                          for (final entry in all)
                            ListTile(
                              title: Text(names[entry.shareId] ?? entry.shareId),
                              subtitle: Text(entry.blocked ? l.sharingConflictHelp : l.sharingPending),
                              trailing: GlassIconButton(
                                icon: Icons.delete_outline,
                                tooltip: l.sharingDiscard,
                                onPressed: () async {
                                  final ok = await showConfirmDialog(
                                    context,
                                    title: l.sharingDiscard,
                                    message: l.sharingDiscardHelp,
                                    confirmLabel: l.sharingDiscard,
                                    destructive: true,
                                  );
                                  if (!ok || !context.mounted || !sharingSessionCurrent(ref, profile)) return;
                                  await runWithFeedback(
                                    context,
                                    () => ref.read(sharingServiceProvider).discardPending(entry.mutationId),
                                  );
                                  if (context.mounted && sharingSessionCurrent(ref, profile)) refreshSharing(ref);
                                },
                              ),
                            ),
                        ],
                      ),
                    ),
            ),
          ),
        ),
        primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );
  }
}

Future<void> showSharingBoundHost(BuildContext context, Host host) =>
    showAppDialog<void>(context, secure: true, builder: (_) => _SharingBoundHost(host));

class _SharingBoundHost extends ConsumerStatefulWidget {
  const _SharingBoundHost(this.host);
  final Host host;
  @override
  ConsumerState<_SharingBoundHost> createState() => _SharingBoundHostState();
}

class _SharingBoundHostState extends ConsumerState<_SharingBoundHost> {
  late final Object? _profile;
  late final Future<List<SharingItem>> _items;
  bool _busy = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _items = ref.read(sharingServiceProvider).list(refresh: true);
  }

  Future<void> _refresh(SharingItem item) async {
    if (!sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    final d = item.data!;
    final result = await runWithFeedback(
      context,
      () => ref
          .read(sharingServiceProvider)
          .refreshBoundHost(
            widget.host.id.value,
            expectedAddress: d['address'] as String,
            expectedPort: d['port'] as int,
          ),
    );
    if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
    if (result != null) {
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => SharingDialogGuard(
    profile: _profile,
    child: FutureBuilder<List<SharingItem>>(
      future: _items,
      builder: (context, snapshot) {
        final item = snapshot.data
            ?.where(
              (i) =>
                  i.id == widget.host.metadata['cc.shared.share'] &&
                  i.trust == SharingTrust.verified &&
                  i.kind == SharingKind.host,
            )
            .firstOrNull;
        final l = context.l10n;
        final previousEndpoint = '${widget.host.address}:${widget.host.port ?? defaultSshPort}';
        final nextEndpoint = item == null ? null : '${item.data!['address']}:${item.data!['port']}';
        return GlassDialog(
          title: l.sharingRefreshHost,
          content: SizedBox(
            width: 540,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (!snapshot.hasData && !snapshot.hasError) const LinearProgressIndicator(),
                if (snapshot.hasError || (snapshot.hasData && item == null)) Text(l.sharingConflictHelp),
                if (item != null) ...[
                  Text(l.sharingRefreshHostHelp),
                  const SizedBox(height: 12),
                  SelectableText('$previousEndpoint → $nextEndpoint'),
                  const SizedBox(height: 12),
                  SharingProjectionView(item.previewJson!),
                ],
              ],
            ),
          ),
          secondaryActions: [
            GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
          ],
          primaryAction: GlassButton.prominent(
            label: l.commonSave,
            onPressed: _busy || item == null ? null : () => _refresh(item),
          ),
        );
      },
    ),
  );
}
