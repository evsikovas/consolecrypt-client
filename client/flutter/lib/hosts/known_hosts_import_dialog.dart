import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/hosts/known_hosts_import.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KnownHostsImportDialog extends ConsumerStatefulWidget {
  const KnownHostsImportDialog({super.key, this.groupId});
  final ObjectId? groupId;
  @override
  ConsumerState<KnownHostsImportDialog> createState() => _KnownHostsImportDialogState();
}

class _KnownHostsImportDialogState extends ConsumerState<KnownHostsImportDialog> {
  KnownHostsScan? _scan;
  final _selected = <String>{};
  final _existing = <String>{};
  bool _busy = false;
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
    final inventory = ref.read(inventoryServiceProvider);
    final hosts = await ref.read(hostsProvider.future);
    for (final host in hosts.where((h) => !h.isRdp)) {
      if (!_current) return;
      final effective = await inventory.resolveEffective(host);
      _existing.add(KnownHostCandidate(host.address, (effective.port.value ?? 22)).identity);
    }
  }

  Future<void> _load() async {
    try {
      final result = await ref.read(knownHostsImportServiceProvider).scan();
      if (!_current) return;
      await _existingHosts();
      if (!_current) return;
      setState(() {
        _scan = result;
        _selected.addAll(result.hosts.map((h) => h.identity).where((id) => !_existing.contains(id)));
      });
    } catch (_) {
      if (_current) setState(() => _failure = context.l10n.hostImportReadFailed);
    }
  }

  Future<void> _save() async {
    if (_busy || !_current || _scan == null) return;
    setState(() {
      _busy = true;
      _failure = null;
    });
    final inventory = ref.read(inventoryServiceProvider);
    try {
      await _existingHosts();
      for (final candidate in _scan!.hosts) {
        if (!_current) return;
        if (!_selected.contains(candidate.identity) || _existing.contains(candidate.identity)) continue;
        final now = DateTime.now().toUtc();
        await inventory.saveHost(
          Host(
            id: ObjectId.generate(),
            name: candidate.label,
            address: candidate.address,
            port: candidate.port,
            groupId: widget.groupId,
            createdAt: now,
            updatedAt: now,
          ),
        );
        _saved++;
        _existing.add(candidate.identity);
        _selected.remove(candidate.identity);
      }
      if (mounted && _current) closeDialog(context, _saved);
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
    final result = _scan;
    final available = result?.hosts.where((h) => !_existing.contains(h.identity)).toList() ?? [];
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        key: const ValueKey('known-hosts-import'),
        title: l.hostImportTitle,
        width: 640,
        content: SizedBox(
          height: 420,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.hostImportHelp),
              const SizedBox(height: 12),
              if (_failure != null) Text(_failure!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              if (_saved > 0) Text(l.hostImportSaved(_saved)),
              if (result == null && _failure == null) const LinearProgressIndicator(),
              if (result != null) ...[
                if (result.skipped > 0) Text(l.hostImportSkipped(result.skipped)),
                if (result.unreadable.isNotEmpty) ...[Text(l.hostImportReadFailed), Text(result.unreadable.join('\n'))],
                if (available.isNotEmpty)
                  CheckboxListTile(
                    title: Text(l.hostImportSelectAll),
                    value: available.every((h) => _selected.contains(h.identity)),
                    onChanged: _busy
                        ? null
                        : (checked) => setState(() {
                            if (checked == true) {
                              _selected.addAll(available.map((h) => h.identity));
                            } else {
                              _selected.clear();
                            }
                          }),
                  ),
                Expanded(
                  child: ListView(
                    children: [
                      if (result.hosts.isEmpty) ListTile(title: Text(l.hostImportEmpty)),
                      for (final host in result.hosts)
                        CheckboxListTile(
                          key: ValueKey('import-host-${host.identity}'),
                          title: Text(host.label),
                          subtitle: _existing.contains(host.identity) ? Text(l.hostImportExists) : null,
                          value: _selected.contains(host.identity) && !_existing.contains(host.identity),
                          onChanged: _busy || _existing.contains(host.identity)
                              ? null
                              : (checked) => setState(() {
                                  if (checked == true) {
                                    _selected.add(host.identity);
                                  } else {
                                    _selected.remove(host.identity);
                                  }
                                }),
                        ),
                      for (final source in result.sources)
                        Padding(padding: const EdgeInsets.all(8), child: Text(source)),
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
          key: const ValueKey('known-hosts-import-save'),
          label: l.commonImport,
          busy: _busy,
          onPressed: _busy || _scan == null || _selected.isEmpty ? null : _save,
        ),
      ),
    );
  }
}
