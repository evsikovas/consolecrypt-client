import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';

import 'package:consolecrypt/groups/group_tree.dart';
import 'package:consolecrypt/hosts/jump_chain_editor.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showGroupEditor(BuildContext context, {Group? group, ObjectId? parentId}) => showAppDialog<void>(
  context,
  builder: (_) => _GroupDialog(group: group, parentId: parentId),
);

Future<void> showJumpProfiles(BuildContext context) => showAppDialog<void>(
  context,
  builder: (context) => GlassDialog(
    title: context.l10n.groupsJumpProfilesSection,
    width: 560,
    content: const SingleChildScrollView(child: _JumpProfilesCard()),
    secondaryActions: [GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context))],
  ),
);

class _GroupDefaults extends ConsumerWidget {
  const _GroupDefaults(this.group);
  final Group group;
  String _source(AppLocalizations l, Resolved<Object?> r) => switch (r.source) {
    ValueSource.host => l.groupsSourceOwn,
    ValueSource.group => l.groupsSourceInherited(r.sourceName ?? ''),
    _ => l.groupsSourceUnset,
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(groupsProvider);
    ref.watch(credentialsProvider);
    ref.watch(jumpProfilesProvider);
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    return FutureBuilder<EffectiveGroupDefaults>(
      future: ref.read(inventoryServiceProvider).resolveGroupDefaults(group.id),
      builder: (context, snap) {
        final d = snap.data;
        if (d == null) return const LinearProgressIndicator();
        Widget row(String label, String? value, Resolved<Object?> r) => LabeledValue(
          label: label,
          value: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(value ?? '—'),
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
          children: [
            row(l10n.groupsUsername, d.username.value, d.username),
            row(l10n.groupsPort, d.port.value?.toString(), d.port),
            row(l10n.groupsCredential, d.credentialName, d.credentialId),
            row(l10n.groupsJumpProfile, d.jumpProfileName, d.jumpProfileId),
            if (group.tags.isNotEmpty)
              LabeledValue(
                label: l10n.tagsLabel,
                value: TagChips(tags: group.tags),
              ),
          ],
        );
      },
    );
  }
}

class _GroupDialog extends ConsumerStatefulWidget {
  const _GroupDialog({this.group, this.parentId});

  final Group? group;
  final ObjectId? parentId;

  @override
  ConsumerState<_GroupDialog> createState() => _GroupDialogState();
}

class _GroupDialogState extends ConsumerState<_GroupDialog> {
  late final TextEditingController _name = TextEditingController(text: widget.group?.name ?? '');
  late final TextEditingController _username = TextEditingController(text: widget.group?.inheritedUsername ?? '');
  late final TextEditingController _port = TextEditingController(text: widget.group?.inheritedPort?.toString() ?? '');
  late ObjectId? _parent = widget.group?.parentId ?? widget.parentId;
  late ObjectId? _credential = widget.group?.inheritedCredentialId;
  int _credentialEpoch = 0;
  late ObjectId? _jump = widget.group?.inheritedJumpProfileId;
  late List<String> _tags = [...?widget.group?.tags];

  @override
  void dispose() {
    _name.dispose();
    _username.dispose();
    _port.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final now = DateTime.now().toUtc();
    final g = widget.group;
    final group = Group(
      id: g?.id ?? ObjectId.generate(),
      name: _name.text.trim(),
      parentId: _parent,
      inheritedUsername: _username.text.trim().isEmpty ? null : _username.text.trim(),
      inheritedPort: int.tryParse(_port.text.trim()),
      inheritedCredentialId: _credential,
      inheritedJumpProfileId: _jump,
      tags: _tags,
      createdAt: g?.createdAt ?? now,
      updatedAt: now,
    );
    final saved = await runWithFeedback(context, () => ref.read(inventoryServiceProvider).saveGroup(group));
    if (saved != null && mounted) closeDialog<void>(context);
  }

  @override
  Widget build(BuildContext context) {
    final groups = ref.watch(groupsProvider).value ?? const <Group>[];
    final byId = ref.watch(groupByIdProvider);
    final credentials = ref.watch(credentialsProvider).value ?? const <Credential>[];
    final profiles = ref.watch(jumpProfilesProvider).value ?? const <JumpProfile>[];
    final l10n = context.l10n;
    // Exclude the group itself and its descendants from parent choices.
    final excluded = <ObjectId>{};
    if (widget.group != null) {
      excluded.add(widget.group!.id);
      var grew = true;
      while (grew) {
        grew = false;
        for (final g in groups) {
          if (g.parentId != null && excluded.contains(g.parentId) && excluded.add(g.id)) grew = true;
        }
      }
    }
    final tokens = GlassTokens.of(context);
    final menuRadius = BorderRadius.circular(tokens.radii.menu);
    return GlassDialog(
      key: const ValueKey('group-dialog'),
      title: widget.group == null ? l10n.groupsNewGroup : l10n.groupsEditGroup,
      width: 520,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              key: const ValueKey('group-name'),
              controller: _name,
              autofocus: true,
              decoration: InputDecoration(labelText: l10n.commonName),
            ),
            const SizedBox(height: GlassSpacing.s12),
            DropdownButtonFormField<ObjectId?>(
              isExpanded: true,
              borderRadius: menuRadius,
              initialValue: byId.containsKey(_parent) ? _parent : null,
              decoration: InputDecoration(labelText: l10n.groupsParentGroup),
              items: [
                DropdownMenuItem(value: null, child: Text(l10n.groupsNoParent)),
                for (final g in groups.where((g) => !excluded.contains(g.id)))
                  DropdownMenuItem(value: g.id, child: Text(groupPathName(byId, g.id))),
              ],
              onChanged: (v) => setState(() => _parent = v),
            ),
            const SizedBox(height: GlassSpacing.s16),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: Text(
                l10n.groupsDefaultsHeading,
                style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
              ),
            ),
            const SizedBox(height: GlassSpacing.s8),
            Row(
              children: [
                Expanded(
                  flex: 2,
                  child: TextField(
                    controller: _username,
                    decoration: InputDecoration(labelText: l10n.groupsUsername),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: TextField(
                    controller: _port,
                    keyboardType: TextInputType.number,
                    inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                    decoration: InputDecoration(labelText: l10n.groupsPort),
                  ),
                ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s12),
            DropdownButtonFormField<Object?>(
              key: ValueKey('group-credential-$_credentialEpoch'),
              isExpanded: true,
              borderRadius: menuRadius,
              initialValue: credentials.any((c) => c.id == _credential) ? _credential : null,
              decoration: InputDecoration(labelText: l10n.groupsCredential),
              items: [
                DropdownMenuItem(value: null, child: Text(l10n.groupsInherit)),
                for (final c in credentials) DropdownMenuItem(value: c.id, child: Text(c.name)),
                DropdownMenuItem(value: newCredentialEntry, child: Text(l10n.hostsNewCredentialEntry)),
              ],
              onChanged: (v) async {
                if (v == newCredentialEntry) {
                  final created = await showNewCredentialChooser(context);
                  setState(() {
                    if (created != null) _credential = created.id;
                    _credentialEpoch++;
                  });
                } else {
                  setState(() => _credential = v as ObjectId?);
                }
              },
            ),
            const SizedBox(height: GlassSpacing.s12),
            DropdownButtonFormField<ObjectId?>(
              isExpanded: true,
              borderRadius: menuRadius,
              initialValue: profiles.any((p) => p.id == _jump) ? _jump : null,
              decoration: InputDecoration(labelText: l10n.groupsJumpProfile),
              items: [
                DropdownMenuItem(value: null, child: Text(l10n.groupsJumpInherit)),
                for (final p in profiles) DropdownMenuItem(value: p.id, child: Text(p.name)),
              ],
              onChanged: (v) => setState(() => _jump = v),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TagEditor(tags: _tags, onChanged: (t) => setState(() => _tags = t)),
            if (widget.group != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              ExpansionTile(title: Text(l10n.inventoryEffectiveDefaults), children: [_GroupDefaults(widget.group!)]),
            ],
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(key: const ValueKey('save-group'), onPressed: _save, label: l10n.commonSave),
    );
  }
}

class _JumpProfilesCard extends ConsumerWidget {
  const _JumpProfilesCard();

  Future<void> _edit(BuildContext context, {JumpProfile? profile}) =>
      showAppDialog<void>(context, builder: (_) => _JumpProfileDialog(profile: profile));

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profiles = ref.watch(jumpProfilesProvider).value ?? const <JumpProfile>[];
    final hosts = ref.watch(hostByIdProvider);
    final l10n = context.l10n;
    return SectionCard(
      title: l10n.groupsJumpProfilesSection,
      icon: Icons.alt_route_rounded,
      trailing: GlassIconButton(
        tooltip: l10n.groupsNewJumpProfile,
        icon: Icons.add_rounded,
        style: GlassIconButtonStyle.plain,
        onPressed: () => _edit(context),
      ),
      child: profiles.isEmpty
          ? Text(l10n.groupsJumpProfilesEmpty)
          : Column(
              children: [
                for (final p in profiles)
                  ListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    title: Text(p.name),
                    subtitle: Text(p.chain.map((id) => hosts[id]?.name ?? '?').join(' → ')),
                    onTap: () => _edit(context, profile: p),
                    trailing: GlassIconButton(
                      tooltip: l10n.commonDelete,
                      icon: Icons.delete_rounded,
                      style: GlassIconButtonStyle.plain,
                      iconSize: 18,
                      onPressed: () =>
                          runWithFeedback(context, () => ref.read(inventoryServiceProvider).deleteJumpProfile(p.id)),
                    ),
                  ),
              ],
            ),
    );
  }
}

class _JumpProfileDialog extends ConsumerStatefulWidget {
  const _JumpProfileDialog({this.profile});

  final JumpProfile? profile;

  @override
  ConsumerState<_JumpProfileDialog> createState() => _JumpProfileDialogState();
}

class _JumpProfileDialogState extends ConsumerState<_JumpProfileDialog> {
  late final TextEditingController _name = TextEditingController(text: widget.profile?.name ?? '');
  late List<ObjectId> _chain = [...?widget.profile?.chain];

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final now = DateTime.now().toUtc();
    final p = JumpProfile(
      id: widget.profile?.id ?? ObjectId.generate(),
      name: _name.text.trim(),
      chain: _chain,
      createdAt: widget.profile?.createdAt ?? now,
      updatedAt: now,
    );
    final saved = await runWithFeedback(context, () => ref.read(inventoryServiceProvider).saveJumpProfile(p));
    if (saved != null && mounted) closeDialog<void>(context);
  }

  @override
  Widget build(BuildContext context) {
    final hosts = ref.watch(hostsProvider).value ?? const <Host>[];
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('jump-profile-dialog'),
      title: widget.profile == null ? l10n.groupsNewJumpProfile : l10n.groupsEditJumpProfile,
      width: 520,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: _name,
              decoration: InputDecoration(labelText: l10n.commonName),
            ),
            const SizedBox(height: GlassSpacing.s12),
            JumpChainEditor(chain: _chain, hosts: hosts, onChanged: (c) => setState(() => _chain = c)),
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(onPressed: _save, label: l10n.commonSave),
    );
  }
}
