import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/sharing/peer_verification.dart';
import 'package:consolecrypt/sharing/sharing_database.dart';
import 'package:consolecrypt/sharing/sharing_kubernetes.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

bool sharingSessionCurrent(WidgetRef ref, Object? scope) =>
    scope is SharingSessionScope &&
    scope.unlocked &&
    scope.profile != null &&
    identical(ref.read(sharingSessionScopeProvider), scope);

/// Close trust/plaintext dialogs on lock/profile switch, including dialogs
/// stacked over an indexed shell. Never carry a decision into another profile.
class SharingDialogGuard extends ConsumerWidget {
  const SharingDialogGuard({required this.profile, required this.child, super.key});
  final Object? profile;
  final Widget child;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    void close() {
      if (!sharingSessionCurrent(ref, profile)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!context.mounted) return;
          final route = ModalRoute.of(context);
          // Remove this exact guarded route, including a parent underneath a
          // second trust dialog; never accidentally pop a different route.
          if (route != null && route.isActive) Navigator.of(context).removeRoute(route);
        });
      }
    }

    ref.listen(sharingSessionScopeProvider, (_, _) => close());
    return sharingSessionCurrent(ref, profile) ? child : const SizedBox.shrink();
  }
}

class SharingProjectionView extends StatelessWidget {
  const SharingProjectionView(this.raw, {super.key});
  final String raw;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final j = jsonDecode(raw) as Map<String, dynamic>;
    final d = j['data'] as Map<String, dynamic>;
    final secret = j['kind'] == 'secret';
    final fields = <String, Object?>{
      l.sharingName: d['name'],
      l.sharingAddress: d['address'],
      l.sharingPort: d['port'],
      l.sharingUsername: d['username'],
      l.sharingDescription: d['description'],
      l.sharingCommand: d['template'],
      l.sharingNotes: d['notes'],
      if (j['kind'] == 'kube_context') ...{
        l.sharingKubeCluster: d['cluster_name'],
        l.sharingAddress: d['server'],
        l.kubeNamespace: d['namespace'],
      },
      if (j['kind'] == 'db_connection') ...{
        l.dbEngine: DbEngine.values.where((e) => e.wireName == d['engine']).firstOrNull?.label ?? d['engine'],
        l.sharingAddress: d['host'],
        l.dbDatabase: d['database'],
        l.sharingDbRouteKind: switch (d['route_kind']) {
          'relay' => l.sharingDbRouteKindRelay,
          'kubernetes' => 'Kubernetes', // l10n-ignore: product name
          'docker' => 'Docker', // l10n-ignore: product name
          _ => l.dbDirect,
        },
        l.kubeNamespace: d['route_namespace'],
        l.sharingDbWorkload: d['route_target_name'],
        l.sharingDbContainer: d['route_container'],
      },
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final f in fields.entries)
          if (f.value != null && f.value.toString().isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(f.key, style: Theme.of(context).textTheme.labelMedium),
                  SelectableText(f.value.toString()),
                ],
              ),
            ),
        if (secret) const Text('••••••••'), // l10n-ignore: masked secret
        if (j['kind'] == 'snippet') Text(l.sharingNoAutoRun),
        if (j['kind'] == 'kube_context') ...[
          if (d['production'] == true) Text(l.kubeProduction),
          Text(d['includes_credentials'] == true ? l.sharingKubeCredentialsIncluded : l.sharingKubeCredentialsExcluded),
        ],
        if (j['kind'] == 'db_connection') ...[
          if (d['production'] == true) Text(l.dbProduction, key: const ValueKey('sharing-db-production')),
          Text(d['includes_credentials'] == true ? l.sharingDbCredentialsIncluded : l.sharingDbCredentialsExcluded),
        ],
      ],
    );
  }
}

class SharingRecipients extends ConsumerStatefulWidget {
  const SharingRecipients({
    required this.onChanged,
    this.initialGrants = const [],
    this.excludedDeviceIds = const {},
    this.sensitive = false,
    super.key,
  });
  final ValueChanged<List<SharingGrant>> onChanged;
  final List<SharingGrant> initialGrants;
  final Set<String> excludedDeviceIds;

  /// Secrets, Kubernetes contexts, database connections: the default policy
  /// admits only verified recipients (enforced in the core).
  final bool sensitive;
  @override
  ConsumerState<SharingRecipients> createState() => _SharingRecipientsState();
}

class _SharingRecipientsState extends ConsumerState<SharingRecipients> {
  final _email = TextEditingController();
  final Map<String, SharingGrant> _grants = {};
  List<SharingIdentity> _devices = [];
  String _foundEmail = '';
  bool _busy = false;
  String? _error;
  @override
  void initState() {
    super.initState();
    for (final grant in widget.initialGrants) {
      if (!widget.excludedDeviceIds.contains(grant.identity.deviceId)) _grants[grant.identity.deviceId] = grant;
    }
  }

  @override
  void dispose() {
    _email.clear();
    _email.dispose();
    super.dispose();
  }

  Future<void> _find() async {
    final profile = ref.read(sharingSessionScopeProvider);
    final email = _email.text.trim();
    if (!sharingSessionCurrent(ref, profile) || email.isEmpty) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final devices = await ref.read(sharingServiceProvider).discover(email);
      if (mounted && sharingSessionCurrent(ref, profile) && _email.text.trim() == email) {
        setState(() {
          for (final d in devices) {
            final old = _grants[d.deviceId];
            if (old != null && !old.identity.sameKeys(d)) _grants.remove(d.deviceId);
          }
          _devices = devices;
          _foundEmail = email;
          widget.onChanged(_grants.values.toList(growable: false));
          _error = devices.isEmpty ? context.l10n.sharingNoDevices : null;
        });
      }
    } catch (e) {
      // Recipient lookups are rate limited (protocol 1.11): say when to retry.
      if (mounted) setState(() => _error = rateLimitedMessage(context.l10n, e) ?? context.l10n.sharingNoDevices);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _replace(SharingIdentity identity) {
    _devices = [for (final d in _devices) d.deviceId == identity.deviceId ? identity : d];
    final old = _grants[identity.deviceId];
    if (old != null) _grants[identity.deviceId] = SharingGrant(identity, old.role);
  }

  /// TR-12: choosing a device pins its keys; a known device with other
  /// keys is never selected.
  Future<void> _choose(SharingIdentity identity, bool selected, [SharingRole? role]) async {
    final profile = ref.read(sharingSessionScopeProvider);
    if (!selected) {
      setState(() => _grants.remove(identity.deviceId));
      widget.onChanged(_grants.values.toList(growable: false));
      return;
    }
    var chosen = identity;
    if (chosen.verificationId == null || chosen.trust == PeerTrust.none) {
      setState(() => _busy = true);
      final pin = await runWithFeedback(
        context,
        () => ref.read(sharingServiceProvider).choosePeer(identity, email: _foundEmail.isEmpty ? null : _foundEmail),
      );
      if (!mounted || !sharingSessionCurrent(ref, profile)) return;
      setState(() => _busy = false);
      if (pin == null) return;
      chosen = identity.withPin(pin);
      setState(() => _replace(chosen));
    }
    if (chosen.trust == PeerTrust.keysChanged) return;
    setState(() => _grants[chosen.deviceId] = SharingGrant(chosen, role ?? SharingRole.reader));
    widget.onChanged(_grants.values.toList(growable: false));
  }

  Future<void> _verify(SharingIdentity identity) async {
    final profile = ref.read(sharingSessionScopeProvider);
    final pin = await showPeerVerification(context, identity, email: _foundEmail.isEmpty ? null : _foundEmail);
    if (pin == null || !mounted || !sharingSessionCurrent(ref, profile)) return;
    setState(() => _replace(identity.withPin(pin)));
    widget.onChanged(_grants.values.toList(growable: false));
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final devices = <String, SharingIdentity>{
      for (final g in _grants.values) g.identity.deviceId: g.identity,
      for (final d in _devices)
        if (!widget.excludedDeviceIds.contains(d.deviceId)) d.deviceId: d,
    }.values;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          key: const ValueKey('sharing-email'),
          controller: _email,
          keyboardType: TextInputType.emailAddress,
          decoration: InputDecoration(labelText: l.sharingEmail),
          onSubmitted: (_) => _busy ? null : _find(),
        ),
        const SizedBox(height: 8),
        GlassButton(
          key: const ValueKey('sharing-find'),
          label: l.sharingFind,
          icon: Icons.person_search_outlined,
          onPressed: _busy ? null : _find,
        ),
        if (_busy) const LinearProgressIndicator(),
        if (_error != null) Padding(padding: const EdgeInsets.only(top: 8), child: Text(_error!)),
        if (_devices.isNotEmpty) ...[
          const SizedBox(height: 12),
          Text(l.sharingSelectDevice),
          if (widget.sensitive) Text(l.sharingVerifiedOnlyHint, key: const ValueKey('sharing-verified-only-hint')),
        ],
        for (final d in devices)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: ContentSurface(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('${l.sharingDevice}: ${d.deviceId}'),
                  const SizedBox(height: 4),
                  Wrap(
                    spacing: 8,
                    runSpacing: 4,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      PeerTrustBadge(d.trust, verifiedAt: d.verifiedAt),
                      if (d.trust != PeerTrust.verified)
                        GlassButton(
                          key: ValueKey('sharing-verify-peer-${d.deviceId}'),
                          label: l.sharingVerifyAction,
                          icon: Icons.verified_user_outlined,
                          onPressed: _busy ? null : () => _verify(d),
                        ),
                    ],
                  ),
                  if (d.trust == PeerTrust.keysChanged)
                    Padding(
                      padding: const EdgeInsets.only(top: 4),
                      child: Text(
                        l.sharingKeysChangedWarning(d.deviceId, _foundEmail.isEmpty ? d.userId : _foundEmail),
                        key: ValueKey('sharing-keys-changed-${d.deviceId}'),
                      ),
                    ),
                  Text('${l.sharingFingerprint}: ${d.code}', style: Theme.of(context).textTheme.bodySmall),
                  CheckboxListTile(
                    key: ValueKey('sharing-give-access-${d.deviceId}'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.sharingGiveAccess),
                    value: _grants.containsKey(d.deviceId),
                    onChanged: _busy || d.trust == PeerTrust.keysChanged ? null : (v) => _choose(d, v == true),
                  ),
                  if (_grants[d.deviceId] case final grant?)
                    GlassSelect<SharingRole>(
                      value: grant.role,
                      items: [
                        GlassSelectItem(value: SharingRole.reader, label: l.sharingReader),
                        GlassSelectItem(value: SharingRole.editor, label: l.sharingEditor),
                      ],
                      onChanged: (role) => _choose(grant.identity, true, role),
                    ),
                  Text(_grants[d.deviceId]?.role == SharingRole.editor ? l.sharingEditorHelp : l.sharingReaderHelp),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

Future<void> showSharingPublish(BuildContext context, {required SharingKind kind, required String objectId}) =>
    switch (kind) {
      SharingKind.kubeContext => showSharingKubeContext(context, contextId: objectId),
      SharingKind.dbConnection => showSharingDbConnection(context, connectionId: objectId),
      _ => showAppDialog<void>(
        context,
        secure: true,
        builder: (_) => SharingPublishDialog(kind: kind, objectId: objectId),
      ),
    };

class SharingPublishDialog extends ConsumerStatefulWidget {
  const SharingPublishDialog({required this.kind, required this.objectId, super.key});
  final SharingKind kind;
  final String objectId;
  @override
  ConsumerState<SharingPublishDialog> createState() => _SharingPublishDialogState();
}

class _SharingPublishDialogState extends ConsumerState<SharingPublishDialog> {
  Object? _profile;
  String? _preview;
  bool _notes = false, _busy = false, _secretConfirmed = false, _ready = false;
  Object? _failure;
  bool get _failed => _failure != null;
  List<SharingGrant> _grants = [];
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _busy = true;
      _preview = null;
      _failure = null;
    });
    try {
      final status = await ref.read(sharingServiceProvider).status();
      if (!status.enabled || status.locked) throw StateError('unavailable');
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      setState(() => _ready = true);
      final p = await ref.read(sharingServiceProvider).preview(widget.kind, widget.objectId, includeNotes: _notes);
      if (mounted && sharingSessionCurrent(ref, _profile)) setState(() => _preview = p);
    } catch (e) {
      if (mounted) setState(() => _failure = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _publish() async {
    if (_preview == null || _grants.isEmpty || !sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    final ok = await runWithFeedback(context, () => ref.read(sharingServiceProvider).publish(_preview!, _grants));
    if (!mounted) return;
    if (ok != null && sharingSessionCurrent(ref, _profile)) {
      // Notes about the host travel with its share (best effort).
      if (widget.kind == SharingKind.host) {
        unawaited(
          ref.read(sharedNoteServiceProvider)?.linkShare('host:${widget.objectId}', ok.id).catchError((Object _) {}),
        );
      }
      // A queued publication already exists even if dispatch fails. Close the
      // composer so retrying delivery cannot accidentally create another share.
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
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        title: l.sharingPublish,
        icon: Icons.share_outlined,
        content: SizedBox(
          width: 600,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_busy) const LinearProgressIndicator(),
                  if (_failed) Text(rateLimitedMessage(l, _failure) ?? l.sharingUnavailable),
                  if (_preview != null) ...[
                    Text(l.sharingPreview, style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 8),
                    SharingProjectionView(_preview!),
                    const SizedBox(height: 8),
                    Text(l.sharingExclude),
                    if (widget.kind == SharingKind.host)
                      CheckboxListTile(
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingIncludeNotes),
                        value: _notes,
                        onChanged: _busy
                            ? null
                            : (v) {
                                _notes = v == true;
                                _load();
                              },
                      ),
                    if (widget.kind == SharingKind.secret) ...[
                      Text(l.sharingSecretWarning),
                      CheckboxListTile(
                        title: Text(l.sharingSecretConfirmed),
                        value: _secretConfirmed,
                        onChanged: (v) => setState(() => _secretConfirmed = v == true),
                      ),
                    ],
                  ],
                  if (_ready) ...[
                    const SizedBox(height: 16),
                    SharingRecipients(
                      key: const ValueKey('sharing-recipients'),
                      sensitive: sharingKindSensitive(widget.kind),
                      onChanged: (g) => setState(() => _grants = g),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-publish-confirm'),
          label: l.sharingPublishConfirm,
          onPressed:
              _busy || _preview == null || _grants.isEmpty || (widget.kind == SharingKind.secret && !_secretConfirmed)
              ? null
              : _publish,
        ),
      ),
    );
  }
}

Future<void> showSharingAccept(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingAcceptDialog(item));

class SharingAcceptDialog extends ConsumerStatefulWidget {
  const SharingAcceptDialog(this.item, {this.reconcile = false, this.enrollmentRecovery = false, super.key});
  final SharingItem item;
  final bool reconcile, enrollmentRecovery;
  @override
  ConsumerState<SharingAcceptDialog> createState() => _SharingAcceptDialogState();
}

class _SharingAcceptDialogState extends ConsumerState<SharingAcceptDialog> {
  Object? _profile;
  late Future<SharingInvitation> _invitation;
  SharingIdentity? _owner;
  bool _busy = false;
  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _invitation = ref.read(sharingServiceProvider).inspect(widget.item.id);
  }

  Future<void> _verify(SharingIdentity owner) async {
    final pin = await showPeerVerification(context, owner);
    if (pin == null || !mounted || !sharingSessionCurrent(ref, _profile)) return;
    setState(() => _owner = owner.withPin(pin));
  }

  /// TR-16: the owner's keys are pinned (chosen on first use, or verified)
  /// and the core accepts only with that pin.
  Future<void> _accept(SharingInvitation invitation) async {
    if (!sharingSessionCurrent(ref, _profile)) return;
    setState(() => _busy = true);
    final accepted = await runWithFeedback(context, () async {
      var owner = _owner ?? invitation.owner;
      if (owner.verificationId == null || owner.trust == PeerTrust.none) {
        owner = owner.withPin(await ref.read(sharingServiceProvider).choosePeer(owner));
      }
      if (mounted) setState(() => _owner = owner);
      final pinId = owner.verificationId!;
      if (widget.enrollmentRecovery) {
        await ref.read(enrollmentServiceProvider).reconcile(invitation.item.id, pinId);
        return true;
      }
      return widget.reconcile
          ? ref.read(sharingServiceProvider).reconcile(invitation.item.id, pinId)
          : ref.read(sharingServiceProvider).accept(invitation.item.id, pinId);
    });
    if (!mounted) return;
    if (accepted != null && sharingSessionCurrent(ref, _profile)) {
      refreshSharing(ref);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SharingDialogGuard(
      profile: _profile,
      child: FutureBuilder<SharingInvitation>(
        future: _invitation,
        builder: (context, snapshot) {
          final owner = _owner ?? snapshot.data?.owner;
          final blocked = owner?.trust == PeerTrust.keysChanged;
          return GlassDialog(
            title: widget.reconcile ? l.sharingReconcile : l.sharingAccept,
            icon: Icons.verified_user_outlined,
            content: SizedBox(
              width: 500,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (!snapshot.hasData && !snapshot.hasError) const LinearProgressIndicator(),
                  if (snapshot.hasError) Text(l.sharingUnavailable),
                  if (owner != null) ...[
                    Text(widget.reconcile ? l.sharingReconcileHelp : l.sharingAcceptOwnerHelp),
                    const SizedBox(height: 12),
                    Text('${l.sharingOwner}: ${owner.userId}'),
                    Text('${l.sharingDevice}: ${owner.deviceId}'),
                    const SizedBox(height: 8),
                    Wrap(
                      spacing: 8,
                      runSpacing: 4,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        PeerTrustBadge(owner.trust, verifiedAt: owner.verifiedAt),
                        if (owner.trust != PeerTrust.verified)
                          GlassButton(
                            key: const ValueKey('sharing-verify-owner'),
                            label: l.sharingVerifyAction,
                            icon: Icons.verified_user_outlined,
                            onPressed: _busy ? null : () => _verify(owner),
                          ),
                      ],
                    ),
                    if (blocked)
                      Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: Text(l.sharingKeysChangedWarning(owner.deviceId, owner.userId)),
                      ),
                    const SizedBox(height: 8),
                    Text('${l.sharingFingerprint}: ${owner.code}', style: Theme.of(context).textTheme.bodySmall),
                    const SizedBox(height: 8),
                    Text(l.sharingNoAutoRun),
                  ],
                ],
              ),
            ),
            secondaryActions: [
              GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
            ],
            primaryAction: GlassButton.prominent(
              key: const ValueKey('sharing-accept-confirm'),
              label: l.sharingAccept,
              onPressed: _busy || snapshot.data == null || blocked ? null : () => _accept(snapshot.data!),
            ),
          );
        },
      ),
    );
  }
}
