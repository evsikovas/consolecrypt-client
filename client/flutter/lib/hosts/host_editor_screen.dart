import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/hosts/host_auth_section.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:consolecrypt/hosts/jump_chain_editor.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

enum JumpMode { inherit, profile, custom }

/// Add/edit a host. Empty port/username/credential inherit from the group
/// chain; the "Effective settings" panel shows what app-core's Connection
/// Planner resolves, with the source of every value.
class HostEditorScreen extends ConsumerStatefulWidget {
  const HostEditorScreen({super.key, this.hostId, this.initialGroupId, this.initialProtocol = HostProtocol.ssh});

  final ObjectId? hostId;
  final ObjectId? initialGroupId;
  final HostProtocol initialProtocol;

  @override
  ConsumerState<HostEditorScreen> createState() => _HostEditorScreenState();
}

class _HostEditorScreenState extends ConsumerState<HostEditorScreen> {
  final _form = GlobalKey<FormState>();
  final _name = TextEditingController();
  final _address = TextEditingController();
  final _port = TextEditingController();
  final _username = TextEditingController();
  final _domain = TextEditingController();
  HostProtocol _protocol = HostProtocol.ssh;
  final _keepalive = TextEditingController();
  final _notes = TextEditingController();

  Host? _original;
  HostAuthDraft? _auth;
  ObjectId? _groupId;
  ObjectId? _jumpProfileId;
  List<ObjectId> _chain = [];
  JumpMode _jumpMode = JumpMode.inherit;
  HostKeyPolicy _policy = HostKeyPolicy.ask;
  SshBackend _backend = SshBackend.native;
  bool _agentForwarding = false;
  List<String> _tags = [];
  late final ObjectId _id;
  late final DateTime _createdAt;

  Future<EffectiveHostConfig>? _effective;

  /// A [ValidationError], [HostAuthDraftError] or [AppException] (turned
  /// into text in `build`).
  Object? _error;
  bool _saving = false;
  bool _loaded = false;

  bool get _isNew => widget.hostId == null;

  @override
  void initState() {
    super.initState();
    _protocol = widget.initialProtocol;
    if (_protocol == HostProtocol.rdp) _port.text = '3389';
    _id = widget.hostId ?? ObjectId.generate();
    _createdAt = DateTime.now().toUtc();
    for (final c in [_name, _address, _port, _username]) {
      c.addListener(_recompute);
    }
  }

  @override
  void dispose() {
    for (final c in [_name, _address, _port, _username, _domain, _keepalive, _notes]) {
      c.dispose();
    }
    _auth?.dispose();
    super.dispose();
  }

  void _load(Host h) {
    _original = h;
    _protocol = h.protocol;
    _domain.text = h.rdpDomain ?? '';
    _name.text = h.name;
    _address.text = h.address;
    _port.text = h.port?.toString() ?? '';
    _username.text = h.username ?? '';
    _keepalive.text = h.keepaliveSecs?.toString() ?? '';
    _notes.text = h.notes;
    _groupId = h.groupId;
    _jumpProfileId = h.jumpProfileId;
    _chain = [...h.jumpChain];
    _jumpMode = h.jumpChain.isNotEmpty
        ? JumpMode.custom
        : (h.jumpProfileId != null ? JumpMode.profile : JumpMode.inherit);
    _policy = h.hostKeyPolicy;
    _backend = h.backend;
    _agentForwarding = h.agentForwarding;
    _tags = [...h.tags];
  }

  /// [forPreview]: reflect the pending authentication so the effective
  /// preview does not show group inheritance the user has overridden.
  Host _draft({bool forPreview = false}) {
    final auth = _auth;
    final previewId = auth?.previewCredentialId(_original);
    final overridesGroup = forPreview && auth != null && auth.mode != HostAuthMode.inherit && previewId == null;
    return Host(
      id: _id,
      name: _name.text.trim(),
      address: _address.text.trim(),
      protocol: _protocol,
      rdpDomain: _domain.text.trim().isEmpty ? null : _domain.text.trim(),
      rdpWidth: _original?.rdpWidth ?? 1280,
      rdpHeight: _original?.rdpHeight ?? 720,
      port: int.tryParse(_port.text.trim()),
      username: _username.text.trim().isEmpty ? null : _username.text.trim(),
      credentialId: forPreview ? previewId : _original?.credentialId,
      groupId: _groupId,
      jumpChain: _protocol == HostProtocol.ssh && _jumpMode == JumpMode.custom ? _chain : const [],
      jumpProfileId: _protocol == HostProtocol.ssh && _jumpMode == JumpMode.profile ? _jumpProfileId : null,
      proxyId: _protocol == HostProtocol.ssh ? _original?.proxyId : null,
      hostKeyPolicy: _policy,
      backend: _protocol == HostProtocol.ssh ? _backend : SshBackend.native,
      keepaliveSecs: int.tryParse(_keepalive.text.trim()),
      agentForwarding: _protocol == HostProtocol.ssh && _agentForwarding,
      tags: _tags,
      notes: _notes.text,
      metadata: overridesGroup
          ? {...?_original?.metadata, HostMetadataKeys.authPrompt: 'password'}
          : (_original?.metadata ?? const {}),
      createdAt: _original?.createdAt ?? _createdAt,
      updatedAt: DateTime.now().toUtc(),
    );
  }

  void _recompute() {
    if (!mounted) return;
    if (_protocol == HostProtocol.rdp) {
      setState(() => _effective = null);
      return;
    }
    final next = ref.read(inventoryServiceProvider).resolveEffective(_draft(forPreview: true));
    setState(() {
      _effective = next;
    });
  }

  void _set(VoidCallback change) {
    setState(change);
    _recompute();
  }

  /// For new hosts: follow the group's credential until the user touches
  /// the Authentication section.
  void _onGroupChanged(ObjectId? groupId, Map<ObjectId, Group> groups) {
    _groupId = groupId;
    var cursor = groupId;
    var inherits = false;
    final seen = <ObjectId>{};
    while (cursor != null && seen.add(cursor)) {
      final g = groups[cursor];
      if (g == null) break;
      if (g.inheritedCredentialId != null) inherits = true;
      cursor = g.parentId;
    }
    if (_isNew && _protocol == HostProtocol.ssh) {
      _auth?.setDefaultMode(inherits ? HostAuthMode.inherit : HostAuthMode.password);
    }
  }

  Future<void> _save() async {
    if (!(_form.currentState?.validate() ?? false)) return;
    final host = _draft();
    final invalid = host.validate();
    final draft = _auth!;
    final (auth, authError) = draft.toAuth();
    if (invalid != null || auth == null) {
      setState(() => _error = invalid ?? authError);
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final inventory = ref.read(inventoryServiceProvider);
    try {
      if (draft.mode == HostAuthMode.sshKey && draft.rememberPassphrase && draft.keyPassphrase.text.isNotEmpty) {
        final passphrase = SecretText(draft.keyPassphrase.text);
        try {
          await inventory.rememberKeyPassphrase(draft.keyCredentialId!, passphrase);
        } finally {
          passphrase.wipe();
        }
        draft.keyPassphrase.clear();
      }
      await inventory.saveHostWithAuth(host, auth);
      draft.password.clear();
      if (mounted) context.go(AppRoutes.hosts);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (auth is HostAuthInlinePassword) auth.password?.wipe();
      if (mounted) setState(() => _saving = false);
    }
  }

  String? _validatePort(String? v) {
    final text = (v ?? '').trim();
    if (text.isEmpty) return null;
    final p = int.tryParse(text);
    return p == null || p < 1 || p > 65535 ? '1–65535' : null;
  }

  @override
  Widget build(BuildContext context) {
    final hosts = ref.watch(hostsProvider).value ?? const <Host>[];
    final groups = ref.watch(groupsProvider).value ?? const <Group>[];
    final groupsById = ref.watch(groupByIdProvider);
    final credentialsById = ref.watch(credentialByIdProvider);
    final profiles = ref.watch(jumpProfilesProvider).value ?? const <JumpProfile>[];
    final l10n = context.l10n;
    if (!_loaded) {
      final existing = hosts.where((h) => h.id == widget.hostId).firstOrNull;
      if (!_isNew && existing == null) {
        return PageScaffold(
          title: l10n.hostEditorLoadingTitle,
          body: const Center(child: CircularProgressIndicator()),
        );
      }
      if (existing != null) _load(existing);
      _auth = HostAuthDraft.forHost(existing, credentialsById);
      if (_isNew && groupsById.containsKey(widget.initialGroupId)) {
        _onGroupChanged(widget.initialGroupId, groupsById);
      }
      _loaded = true;
      scheduleMicrotask(_recompute);
    }
    final wide = MediaQuery.sizeOf(context).width > 1250;
    final form = _buildForm(context, hosts, groups, groupsById, profiles);
    final effective = _protocol == HostProtocol.rdp
        ? SectionCard(
            title: _protocol.name.toUpperCase(),
            icon: Icons.desktop_windows_rounded,
            child: Text(l10n.hostRdpConnectionHelp),
          )
        : _EffectivePanel(future: _effective, credentialOverride: _auth?.previewLabel(l10n, credentialsById));
    final originalName = _original?.name;
    return PageScaffold(
      title: _isNew
          ? l10n.hostEditorNewTitle
          : (originalName == null ? l10n.hostEditorEditTitleFallback : l10n.hostEditorEditTitle(originalName)),
      actions: [
        GlassButton(onPressed: () => context.go(AppRoutes.hosts), label: l10n.commonCancel),
        GlassButton.prominent(
          key: const ValueKey('save-host'),
          busy: _saving,
          onPressed: _saving ? null : _save,
          label: l10n.commonSave,
        ),
      ],
      body: wide
          ? Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  flex: 3,
                  child: ScrollEdgeEffect(child: SingleChildScrollView(child: form)),
                ),
                const SizedBox(width: GlassSpacing.s20),
                Expanded(
                  flex: 2,
                  child: ScrollEdgeEffect(child: SingleChildScrollView(child: effective)),
                ),
              ],
            )
          : ScrollEdgeEffect(
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    form,
                    const SizedBox(height: GlassSpacing.s16),
                    effective,
                  ],
                ),
              ),
            ),
    );
  }

  Widget _buildForm(
    BuildContext context,
    List<Host> hosts,
    List<Group> groups,
    Map<ObjectId, Group> groupsById,
    List<JumpProfile> profiles,
  ) {
    final tokens = GlassTokens.of(context);
    final menuRadius = BorderRadius.circular(tokens.radii.menu);
    final l10n = context.l10n;
    final sortedGroups = [...groups]
      ..sort((a, b) => groupPathName(groupsById, a.id).compareTo(groupPathName(groupsById, b.id)));
    return Form(
      key: _form,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SectionCard(
            title: l10n.hostProtocolLabel,
            icon: Icons.swap_horiz_rounded,
            child: GlassSegmented<HostProtocol>(
              key: const ValueKey('host-protocol'),
              inChrome: false,
              expand: true,
              segments: const [
                GlassSegment(value: HostProtocol.ssh, label: 'SSH'),
                GlassSegment(value: HostProtocol.rdp, label: 'RDP'), // l10n-ignore: protocol identifier
              ],
              selected: _protocol,
              onChanged: !_isNew
                  ? null
                  : (protocol) => _set(() {
                      _protocol = protocol;
                      _port.text = protocol == HostProtocol.rdp ? '3389' : '';
                      _auth?.dispose();
                      _auth = HostAuthDraft.forHost(null, const {});
                    }),
            ),
          ),
          const SizedBox(height: GlassSpacing.s16),
          SectionCard(
            title: l10n.hostEditorConnectionSection,
            icon: Icons.dns_rounded,
            child: Column(
              children: [
                TextFormField(
                  key: const ValueKey('host-name'),
                  controller: _name,
                  decoration: InputDecoration(labelText: l10n.commonName, hintText: 'prod-db-1'),
                  validator: (v) => (v ?? '').trim().isEmpty ? l10n.hostEditorNameRequired : null,
                ),
                const SizedBox(height: GlassSpacing.s12),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      flex: 3,
                      child: TextFormField(
                        key: const ValueKey('host-address'),
                        controller: _address,
                        autocorrect: false,
                        decoration: InputDecoration(
                          labelText: l10n.hostEditorAddressLabel,
                          hintText: l10n.hostEditorAddressHint,
                        ),
                        validator: (v) {
                          final t = (v ?? '').trim();
                          if (t.isEmpty) return l10n.hostEditorAddressRequired;
                          if (RegExp(r'\s').hasMatch(t)) return l10n.hostEditorAddressNoSpaces;
                          return null;
                        },
                      ),
                    ),
                    const SizedBox(width: GlassSpacing.s12),
                    Expanded(
                      child: TextFormField(
                        key: const ValueKey('host-port'),
                        controller: _port,
                        keyboardType: TextInputType.number,
                        inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                        decoration: InputDecoration(
                          labelText: l10n.hostEditorPortLabel,
                          hintText: _protocol == HostProtocol.rdp ? '3389' : l10n.hostEditorPortHint,
                        ),
                        validator: _validatePort,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: GlassSpacing.s12),
                TextFormField(
                  key: const ValueKey('host-username'),
                  controller: _username,
                  validator: (v) =>
                      _protocol == HostProtocol.rdp && (v ?? '').trim().isEmpty ? l10n.hostRdpUsernameRequired : null,
                  autocorrect: false,
                  decoration: InputDecoration(
                    labelText: l10n.hostEditorUsernameLabel,
                    hintText: l10n.hostEditorUsernameHint,
                  ),
                ),
                const SizedBox(height: GlassSpacing.s12),
                if (_protocol == HostProtocol.rdp) ...[
                  TextFormField(
                    key: const ValueKey('host-rdp-domain'),
                    controller: _domain,
                    autocorrect: false,
                    decoration: InputDecoration(labelText: l10n.rdpDomain),
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                ],
                DropdownButtonFormField<ObjectId?>(
                  isExpanded: true,
                  borderRadius: menuRadius,
                  key: const ValueKey('host-group'),
                  initialValue: groups.any((g) => g.id == _groupId) ? _groupId : null,
                  decoration: InputDecoration(labelText: l10n.hostEditorGroupLabel),
                  items: [
                    DropdownMenuItem(value: null, child: Text(l10n.hostEditorNoGroup)),
                    for (final g in sortedGroups)
                      DropdownMenuItem(value: g.id, child: Text(groupPathName(groupsById, g.id))),
                  ],
                  onChanged: (v) => _set(() => _onGroupChanged(v, groupsById)),
                ),
              ],
            ),
          ),
          const SizedBox(height: GlassSpacing.s16),
          HostAuthSection(
            passwordOnly: _protocol == HostProtocol.rdp,
            draft: _auth!,
            onChanged: _recompute,
            groupName: _groupId == null ? null : groupPathName(groupsById, _groupId),
          ),
          const SizedBox(height: GlassSpacing.s16),
          if (_protocol == HostProtocol.ssh) ...[
            SectionCard(
              title: l10n.hostEditorJumpSection,
              icon: Icons.alt_route_rounded,
              subtitle: l10n.hostEditorJumpSubtitle,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  GlassSegmented<JumpMode>(
                    key: const ValueKey('jump-mode'),
                    inChrome: false,
                    expand: true,
                    segments: [
                      GlassSegment(value: JumpMode.inherit, label: l10n.hostEditorJumpInherit),
                      GlassSegment(value: JumpMode.profile, label: l10n.hostEditorJumpProfile),
                      GlassSegment(value: JumpMode.custom, label: l10n.hostEditorJumpCustom),
                    ],
                    selected: _jumpMode,
                    onChanged: (m) => _set(() => _jumpMode = m),
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                  switch (_jumpMode) {
                    JumpMode.inherit => Text(
                      l10n.hostEditorJumpInheritHelp,
                      style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
                    ),
                    JumpMode.profile => DropdownButtonFormField<ObjectId?>(
                      isExpanded: true,
                      borderRadius: menuRadius,
                      initialValue: profiles.any((p) => p.id == _jumpProfileId) ? _jumpProfileId : null,
                      decoration: InputDecoration(labelText: l10n.hostEditorJumpProfile),
                      items: [
                        for (final p in profiles)
                          DropdownMenuItem(
                            value: p.id,
                            child: Text(l10n.hostEditorJumpProfileItem(p.chain.length, p.name)),
                          ),
                      ],
                      onChanged: (v) => _set(() => _jumpProfileId = v),
                    ),
                    JumpMode.custom => JumpChainEditor(
                      chain: _chain,
                      hosts: hosts.where((h) => h.id != _id && !h.isRdp).toList(),
                      targetLabel: _name.text.isEmpty ? l10n.hostEditorThisHost : _name.text,
                      onChanged: (c) => _set(() => _chain = c),
                    ),
                  },
                ],
              ),
            ),
            const SizedBox(height: GlassSpacing.s16),
            SectionCard(
              title: l10n.hostEditorSecuritySection,
              icon: Icons.verified_user_rounded,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    l10n.hostEditorHostKeyPolicy,
                    style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                  ),
                  const SizedBox(height: GlassSpacing.s6),
                  GlassSegmented<HostKeyPolicy>(
                    key: const ValueKey('host-key-policy'),
                    inChrome: false,
                    expand: true,
                    segments: [for (final p in HostKeyPolicy.values) GlassSegment(value: p, label: p.localized(l10n))],
                    selected: _policy,
                    onChanged: (p) => setState(() => _policy = p),
                  ),
                  const SizedBox(height: GlassSpacing.s4),
                  Text(
                    _policy.localizedDescription(l10n),
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                  DropdownButtonFormField<SshBackend>(
                    isExpanded: true,
                    borderRadius: menuRadius,
                    initialValue: _backend,
                    decoration: InputDecoration(labelText: l10n.hostEditorSshBackend),
                    items: [
                      for (final b in SshBackend.values) DropdownMenuItem(value: b, child: Text(b.localized(l10n))),
                    ],
                    onChanged: (v) => setState(() => _backend = v ?? SshBackend.native),
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                  TextFormField(
                    controller: _keepalive,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    decoration: InputDecoration(
                      labelText: l10n.hostEditorKeepalive,
                      hintText: l10n.hostEditorKeepaliveHint,
                    ),
                  ),
                  SwitchListTile.adaptive(
                    contentPadding: EdgeInsets.zero,
                    value: _agentForwarding,
                    onChanged: (v) => setState(() => _agentForwarding = v),
                    title: Text(l10n.hostEditorAgentForwarding),
                    subtitle: Text(l10n.hostEditorAgentForwardingHelp),
                  ),
                ],
              ),
            ),
            const SizedBox(height: GlassSpacing.s16),
          ],
          SectionCard(
            title: l10n.hostEditorOrganisationSection,
            icon: Icons.label_rounded,
            child: Column(
              children: [
                TagEditor(tags: _tags, onChanged: (t) => setState(() => _tags = t)),
                const SizedBox(height: GlassSpacing.s12),
                TextField(
                  controller: _notes,
                  minLines: 2,
                  maxLines: 5,
                  decoration: InputDecoration(labelText: l10n.hostEditorNotes),
                ),
              ],
            ),
          ),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s12),
            GateErrorText(
              key: const ValueKey('host-error'),
              text: switch (_error!) {
                final HostAuthDraftError e => e.message(l10n),
                final e => errorMessage(l10n, e),
              },
            ),
          ],
        ],
      ),
    );
  }
}

class _EffectivePanel extends StatelessWidget {
  const _EffectivePanel({required this.future, this.credentialOverride});

  final Future<EffectiveHostConfig>? future;

  /// Pending host-level authentication from the editor (not yet saved).
  final String? credentialOverride;

  /// The planner names a username taken from a credential `credential <name>`.
  static const _credentialSourcePrefix = 'credential ';

  static String _source(AppLocalizations l, Resolved<Object?> r) => switch (r.source) {
    ValueSource.host => switch (r.sourceName) {
      null => l.hostEditorSourceHost,
      final name when name.startsWith(_credentialSourcePrefix) => l.hostEditorSourceCredential(
        name.substring(_credentialSourcePrefix.length),
      ),
      final name => l.hostEditorSourceFrom(name),
    },
    ValueSource.group => l.hostEditorSourceGroup(r.sourceName ?? ''),
    ValueSource.jumpProfile => l.hostEditorSourceJumpProfile(r.sourceName ?? ''),
    ValueSource.appDefault => l.hostEditorSourceDefault,
    ValueSource.unset => l.hostEditorSourceUnset,
  };

  /// The planner reports problems and the prompt-for-password credential as
  /// English diagnostics: known ones are shown localized, anything else
  /// verbatim.
  static String _diagnostic(AppLocalizations l, String text) {
    const jumpHostDeleted = 'A jump host in the chain was deleted'; // l10n-ignore: planner diagnostic
    const selfJump = 'The host cannot jump through itself'; // l10n-ignore: planner diagnostic
    const noCredential =
        'No credential: you will be asked for a password when connecting'; // l10n-ignore: planner diagnostic
    const credentialMissing = 'The selected credential no longer exists'; // l10n-ignore: planner diagnostic
    const noUsername = 'No username: set one on the host or on a group'; // l10n-ignore: planner diagnostic
    const credentialPrompt = 'Password — asked when connecting'; // l10n-ignore: planner diagnostic
    return switch (text) {
      jumpHostDeleted => l.hostEditorProblemJumpHostDeleted,
      selfJump => l.hostEditorProblemSelfJump,
      noCredential => l.hostEditorProblemNoCredential,
      credentialMissing => l.hostEditorProblemCredentialMissing,
      noUsername => l.hostEditorProblemNoUsername,
      credentialPrompt => l.hostEditorCredentialPrompt,
      _ => text,
    };
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    return SectionCard(
      key: const ValueKey('effective-panel'),
      title: l10n.hostEditorEffectiveTitle,
      icon: Icons.merge_type_rounded,
      subtitle: l10n.hostEditorEffectiveSubtitle,
      child: FutureBuilder<EffectiveHostConfig>(
        future: future,
        builder: (context, snap) {
          final e = snap.data;
          if (e == null) return const LinearProgressIndicator();
          Widget row(String label, String value, Resolved<Object?> r) => LabeledValue(
            label: label,
            labelWidth: 110,
            value: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(value),
                Text(
                  _source(l10n, r),
                  style: tokens.typography.callout.copyWith(
                    color: r.isInherited ? tokens.palette.accent : tokens.secondaryLabel,
                  ),
                ),
              ],
            ),
          );
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (e.groupPath.isNotEmpty)
                LabeledValue(
                  label: l10n.hostEditorEffectiveGroup,
                  labelWidth: 110,
                  value: Text(e.groupPath.join(' › ')),
                ),
              row(l10n.hostEditorEffectiveUsername, e.username.value ?? '—', e.username),
              row(l10n.hostEditorEffectivePort, '${e.port.value ?? defaultSshPort}', e.port),
              if (credentialOverride != null)
                row(l10n.hostEditorEffectiveCredential, credentialOverride!, const Resolved(null, ValueSource.host))
              else
                row(l10n.hostEditorEffectiveCredential, switch (e.credentialName) {
                  final name? => _diagnostic(l10n, name),
                  null => '—',
                }, e.credentialId),
              row(
                l10n.hostEditorEffectiveRoute,
                e.route.isEmpty ? l10n.hostEditorRouteDirect : e.route.map((h) => h.label).join('  →  '),
                e.routeSource,
              ),
              for (final p in e.problems.where(
                (p) => credentialOverride == null || !RegExp('credential|password', caseSensitive: false).hasMatch(p),
              ))
                Padding(
                  padding: const EdgeInsets.only(top: GlassSpacing.s8),
                  child: InfoBanner(tone: BannerTone.warning, message: _diagnostic(l10n, p)),
                ),
            ],
          );
        },
      ),
    );
  }
}
