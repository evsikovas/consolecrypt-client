import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/ssh_config_import_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Imports `Host` entries of `~/.ssh/config`: address, port, user, jump
/// hosts (ProxyJump, imported with the host) and, optionally, the
/// `IdentityFile` key. Returns the number of hosts created.
class SshConfigImportDialog extends ConsumerStatefulWidget {
  const SshConfigImportDialog({super.key, this.groupId});
  final ObjectId? groupId;
  @override
  ConsumerState<SshConfigImportDialog> createState() => _SshConfigImportDialogState();
}

class _SshConfigImportDialogState extends ConsumerState<SshConfigImportDialog> {
  SshConfigPreview? _preview;
  final _selected = <String>{};
  final _existing = <String, Host>{};
  final _keyFailures = <String>[];
  bool _busy = false, _keys = true;
  String? _failure;
  ProfileId? _profile;
  int _saved = 0;

  bool get _current =>
      mounted && ref.read(appStageProvider) == AppStage.unlocked && _profile == ref.read(activeProfileProvider)?.id;

  @override
  void initState() {
    super.initState();
    _profile = ref.read(activeProfileProvider)?.id;
    _load();
  }

  Future<void> _existingHosts() async {
    final hosts = await ref.read(hostsProvider.future);
    _existing
      ..clear()
      ..addEntries(hosts.where((h) => !h.isRdp).map((h) => MapEntry(h.name, h)));
  }

  Future<void> _load() async {
    final service = ref.read(inventoryServiceProvider);
    try {
      if (service is! SshConfigImportService) throw StateError('unsupported'); // l10n-ignore: diagnostic
      final preview = await (service as SshConfigImportService).previewSshConfig();
      if (!_current) return;
      await _existingHosts();
      if (!_current) return;
      setState(() {
        _preview = preview;
        _selected.addAll(
          preview.hosts.where((h) => !h.jumpOnly && !_existing.containsKey(h.alias)).map((h) => h.alias),
        );
      });
    } catch (_) {
      if (_current) setState(() => _failure = context.l10n.hostImportReadFailed);
    }
  }

  /// Selected hosts plus the jump hosts they need, hops first.
  List<SshConfigHostPreview> _plan() {
    final byAlias = {for (final host in _preview!.hosts) host.alias: host};
    final ordered = <SshConfigHostPreview>[];
    final seen = <String>{};
    void add(String alias) {
      final host = byAlias[alias];
      if (host == null || !seen.add(alias)) return;
      host.jump.forEach(add);
      ordered.add(host);
    }

    _selected.forEach(add);
    return ordered;
  }

  Future<ObjectId?> _key(String path, String? username, Map<String, ObjectId?> imported) async {
    if (imported.containsKey(path)) return imported[path];
    final inventory = ref.read(inventoryServiceProvider);
    ObjectId? id;
    try {
      final file = File(path);
      // Private keys are a few KiB; anything larger is not a key.
      if (await file.length() > 64 * 1024) throw const FormatException('too large'); // l10n-ignore: diagnostic
      final key = SecretText(await file.readAsString());
      try {
        final credential = await inventory.importKeyCredential(
          name: file.uri.pathSegments.lastOrNull ?? path,
          privateKey: key,
          username: username,
        );
        id = credential.id;
      } finally {
        key.wipe();
      }
    } catch (_) {
      _keyFailures.add(path);
    }
    imported[path] = id;
    return id;
  }

  Future<void> _save() async {
    if (_busy || !_current || _preview == null) return;
    setState(() {
      _busy = true;
      _failure = null;
      _keyFailures.clear();
    });
    final inventory = ref.read(inventoryServiceProvider);
    final keys = <String, ObjectId?>{};
    try {
      await _existingHosts();
      final ids = {for (final entry in _existing.entries) entry.key: entry.value.id};
      for (final entry in _plan()) {
        if (!_current) return;
        if (ids.containsKey(entry.alias)) continue;
        final identity = entry.identityFile;
        final credential = _keys && identity != null ? await _key(identity, entry.username, keys) : null;
        if (!_current) return;
        final now = DateTime.now().toUtc();
        final saved = await inventory.saveHost(
          Host(
            id: ObjectId.generate(),
            name: entry.alias,
            address: entry.address,
            port: entry.port,
            username: entry.username,
            credentialId: credential,
            groupId: widget.groupId,
            jumpChain: [for (final hop in entry.jump) ?ids[hop]],
            keepaliveSecs: entry.keepaliveSecs,
            agentForwarding: entry.agentForwarding,
            createdAt: now,
            updatedAt: now,
          ),
        );
        ids[entry.alias] = saved.id;
        _saved++;
      }
      await _existingHosts();
      if (!_current) return;
      if (_keyFailures.isEmpty) {
        if (mounted) closeDialog(context, _saved);
      } else {
        setState(() => _selected.clear());
      }
    } on AppException catch (error) {
      if (_current) setState(() => _failure = errorMessage(context.l10n, error));
    } catch (_) {
      if (_current) setState(() => _failure = context.l10n.hostImportSaveFailed);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    void closeIfStale() {
      if (_current) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    }

    ref.listen(appStageProvider, (_, _) => closeIfStale());
    ref.listen(activeProfileProvider, (_, _) => closeIfStale());
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final preview = _preview;
    final hosts = preview?.hosts.where((h) => !h.jumpOnly).toList() ?? const <SshConfigHostPreview>[];
    final available = hosts.where((h) => !_existing.containsKey(h.alias)).toList();
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        key: const ValueKey('ssh-config-import'),
        title: l.sshConfigImportTitle,
        width: 680,
        content: SizedBox(
          height: 460,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.sshConfigImportHelp),
              const SizedBox(height: GlassSpacing.s8),
              if (_failure != null) Text(_failure!, style: TextStyle(color: tokens.palette.danger)),
              if (_saved > 0) Text(l.hostImportSaved(_saved)),
              for (final path in _keyFailures)
                Text(l.sshConfigKeyFailed(path), style: TextStyle(color: tokens.palette.warning)),
              if (preview == null && _failure == null) const LinearProgressIndicator(),
              if (preview != null && !preview.found) Text(l.sshConfigNotFound(preview.path)),
              if (preview != null && preview.found) ...[
                CheckboxListTile(
                  key: const ValueKey('ssh-config-import-keys'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l.sshConfigImportKeys),
                  subtitle: Text(l.sshConfigImportKeysHelp),
                  value: _keys,
                  onChanged: _busy ? null : (value) => setState(() => _keys = value ?? false),
                ),
                if (available.isNotEmpty)
                  CheckboxListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.hostImportSelectAll),
                    value: available.every((h) => _selected.contains(h.alias)),
                    onChanged: _busy
                        ? null
                        : (checked) => setState(() {
                            if (checked == true) {
                              _selected.addAll(available.map((h) => h.alias));
                            } else {
                              _selected.clear();
                            }
                          }),
                  ),
                Expanded(
                  child: ListView(
                    children: [
                      if (hosts.isEmpty) ListTile(title: Text(l.hostImportEmpty)),
                      for (final host in hosts)
                        CheckboxListTile(
                          key: ValueKey('ssh-config-host-${host.alias}'),
                          contentPadding: EdgeInsets.zero,
                          title: Text(host.alias),
                          subtitle: Text(
                            [
                              '${host.username == null ? '' : '${host.username}@'}${host.address}${host.port == null ? '' : ':${host.port}'}',
                              if (host.jump.isNotEmpty) l.sshConfigVia(host.jump.join(' → ')),
                              ?host.identityFile,
                              if (host.proxyCommand) l.sshConfigProxyCommand,
                              if (_existing.containsKey(host.alias)) l.hostImportExists,
                            ].join(' · '),
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                          ),
                          value: _selected.contains(host.alias) && !_existing.containsKey(host.alias),
                          onChanged: _busy || _existing.containsKey(host.alias)
                              ? null
                              : (checked) => setState(() {
                                  if (checked == true) {
                                    _selected.add(host.alias);
                                  } else {
                                    _selected.remove(host.alias);
                                  }
                                }),
                        ),
                      for (final warning in preview.warnings.take(20))
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: 2),
                          child: Text(warning, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                        ),
                    ],
                  ),
                ),
              ],
            ],
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, _saved)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('ssh-config-import-save'),
          label: l.commonImport,
          busy: _busy,
          onPressed: _busy || preview == null || _selected.isEmpty ? null : _save,
        ),
      ),
    );
  }
}
