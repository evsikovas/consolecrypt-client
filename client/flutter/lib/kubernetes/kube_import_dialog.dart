import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:consolecrypt/kubernetes/kube_import_exec_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:desktop_drop/desktop_drop.dart';
import 'package:file_selector/file_selector.dart' as files;
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String _authenticationLabel(AppLocalizations l, String? kind) => switch (kind) {
  'token' => l.kubeAuthToken,
  'client_certificate' => l.kubeAuthCertificate,
  'oidc' => 'OIDC',
  'exec' => l.kubeAuthExec,
  _ => l.kubeAuthNone,
};

class KubeImportDialog extends ConsumerStatefulWidget {
  const KubeImportDialog({this.initialYaml, this.initialPaths = const [], super.key});
  final SecretText? initialYaml;
  final List<String> initialPaths;
  @override
  ConsumerState<KubeImportDialog> createState() => _KubeImportDialogState();
}

class _KubeImportDialogState extends ConsumerState<KubeImportDialog> {
  final _yaml = TextEditingController();
  final _selected = <int>{};
  final _nonProduction = <int>{};
  final _sshRoutes = <int, String>{};
  late final KubeScope _scope;
  late final KubeService _service;
  KubeImportPreview? _preview;
  Timer? _expiry;
  bool _busy = false, _filesApproved = false, _expired = false;
  String? _failure, _currentFile;
  final _queuedFiles = <String>[];
  bool _savedAny = false;

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  List<KubeImportEntry> get _chosen => _preview?.entries.where((e) => _selected.contains(e.index)).toList() ?? [];
  Set<String> get _files => _chosen.expand((entry) => entry.pendingFiles).toSet();
  bool get _canSave =>
      _current &&
      !_busy &&
      !_expired &&
      _chosen.isNotEmpty &&
      _chosen.length <= 64 &&
      (_files.isEmpty || _filesApproved) &&
      _chosen.every(
        (entry) =>
            entry.error == null &&
            (!entry.insecureTls || _nonProduction.contains(entry.index)) &&
            (!entry.requiresRoute || _sshRoutes.containsKey(entry.index)),
      );

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _service = ref.read(kubeServiceProvider);
    final yaml = widget.initialYaml;
    if (yaml != null) {
      _yaml.text = yaml.expose();
      yaml.wipe();
    }
    _queuedFiles.addAll(widget.initialPaths.take(64));
    if (_queuedFiles.isNotEmpty) WidgetsBinding.instance.addPostFrameCallback((_) => _nextFile());
  }

  void _discard(KubeImportPreview? preview) {
    if (preview != null) unawaited(_service.discard(preview.ticket).catchError((Object _) {}));
  }

  @override
  void dispose() {
    _expiry?.cancel();
    _yaml.clear();
    _queuedFiles.clear();
    _yaml.dispose();
    _discard(_preview);
    super.dispose();
  }

  Future<void> _load(Future<KubeImportPreview> Function() load) async {
    if (!_current || _busy) return;
    setState(() {
      _busy = true;
      _failure = null;
    });
    try {
      final preview = await load();
      if (!_current) {
        _discard(preview);
        return;
      }
      _discard(_preview);
      _yaml.clear();
      _expiry?.cancel();
      final duration = preview.expiresAt.difference(DateTime.now().toUtc());
      setState(() {
        _preview = preview;
        _filesApproved = false;
        _nonProduction.clear();
        _sshRoutes.clear();
        _selected
          ..clear()
          ..addAll(
            preview.entries
                .where((e) => e.error == null && !e.insecureTls && !e.requiresRoute)
                .take(64)
                .map((e) => e.index),
          );
        _expired = duration.isNegative;
      });
      if (!_expired) {
        _expiry = Timer(duration, () {
          if (mounted) setState(() => _expired = true);
        });
      }
    } on _FileSelectionCancelled {
      // Cancelling the OS file picker leaves the current input untouched.
    } catch (_) {
      if (_current) setState(() => _failure = context.l10n.kubeImportFailed);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _paste() async {
    if (!_current || _busy || _yaml.text.trim().isEmpty) return;
    final secret = SecretText(_yaml.text);
    try {
      await _load(() => _service.preview(secret));
    } finally {
      secret.wipe();
    }
  }

  Future<void> _file() async {
    if (!_current || _busy) return;
    setState(() => _busy = true);
    try {
      // Mobile paths may refer to sandbox copies supplied by the document picker.
      if (AppPlatform.isMobile) {
        final path = await ref.read(fileDialogServiceProvider).chooseOpenFile();
        if (!_current || path == null) return;
        if (_queuedFiles.length < 64) _queuedFiles.add(path);
      } else {
        final selected = await files.openFiles();
        if (!_current) return;
        _queuedFiles.addAll(
          selected.map((file) => file.path).where((path) => path.isNotEmpty).take(64 - _queuedFiles.length),
        );
      }
    } catch (_) {
      if (mounted && _current) _failure = context.l10n.kubeImportFailed;
    } finally {
      if (_current) setState(() => _busy = false);
    }
    if (_current) await _nextFile();
  }

  Future<void> _nextFile() async {
    if (!_current || _busy || _queuedFiles.isEmpty) return;
    final path = _queuedFiles.removeAt(0);
    _currentFile = path;
    await _load(() => _service.previewFile(path));
  }

  Future<void> _scan() async {
    final service = _service;
    if (!_current || _busy || service is! KubeInventoryManagementService) return;
    final directory = await ref.read(fileDialogServiceProvider).chooseDirectory();
    if (!_current || directory == null) return;
    setState(() => _busy = true);
    try {
      final paths = await (service as KubeInventoryManagementService).scanConfigFiles(directory);
      if (!mounted || !_current) return;
      _queuedFiles.addAll(paths.take(64 - _queuedFiles.length));
      if (paths.isEmpty) _failure = context.l10n.kubeScanEmpty;
    } catch (_) {
      if (mounted && _current) _failure = context.l10n.kubeImportFailed;
    } finally {
      if (_current) setState(() => _busy = false);
    }
    if (_current) await _nextFile();
  }

  Future<void> _save() async {
    if (!_canSave) return;
    final preview = _preview!;
    final choices = _chosen
        .map(
          (entry) => KubeImportChoice(
            index: entry.index,
            production: !_nonProduction.contains(entry.index),
            approvedFiles: entry.pendingFiles,
            sshHostId: _sshRoutes[entry.index],
          ),
        )
        .toList();
    setState(() {
      _busy = true;
      _failure = null;
    });
    try {
      for (final entry in _chosen.where((entry) => entry.authentication == 'exec')) {
        if (!await _inspectExec(entry.index) || !_current || _expired) return;
      }
      await _service.commit(preview.ticket, choices);
      if (!mounted || !_current) return;
      _preview = null;
      _savedAny = true;
      ref.invalidate(kubeConnectionsProvider);
      if (_queuedFiles.isEmpty) {
        closeDialog(context, true);
      } else {
        setState(() => _busy = false);
        await _nextFile();
      }
    } catch (_) {
      if (_current) {
        setState(() {
          _failure = context.l10n.kubeImportFailed;
          _expired = true;
        });
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<bool> _inspectExec(int index) async {
    final service = _service;
    final preview = _preview;
    if (service is! KubeImportExecService || preview == null || !_current || _expired) return false;
    final inspection = await (service as KubeImportExecService).inspectImportExec(preview.ticket, index);
    try {
      if (!mounted || !_current || _expired) return false;
      return await showAppDialog<bool>(
            context,
            secure: true,
            builder: (_) => KubeImportExecDialog(inspection: inspection, scope: _scope),
          ) ??
          false;
    } finally {
      inspection.dispose();
    }
  }

  void _reset() {
    _discard(_preview);
    _expiry?.cancel();
    setState(() {
      _preview = null;
      _expired = false;
      _failure = null;
      _filesApproved = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _yaml.clear();
      _queuedFiles.clear();
      _discard(_preview);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final hosts = ref.watch(hostsProvider).value?.where((host) => !host.isRdp).toList() ?? [];
    final preview = _preview;
    return PopScope(
      canPop: !_busy,
      child: DropTarget(
        enable: !AppPlatform.isMobile && !_busy,
        onDragDone: (details) {
          if (!_current || _busy) return;
          _queuedFiles.addAll(
            details.files.map((file) => file.path).where((path) => path.isNotEmpty).take(64 - _queuedFiles.length),
          );
          if (_preview == null) {
            unawaited(_nextFile());
          } else {
            setState(() {});
          }
        },
        child: GlassDialog(
          key: const ValueKey('kube-import-dialog'),
          title: l.kubeImport,
          width: 760,
          content: SizedBox(
            height: 520,
            child: SingleChildScrollView(
              key: const ValueKey('kube-import-scroll'),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(l.kubeImportHelp),
                  if (_currentFile != null) Text(_currentFile!, maxLines: 2, overflow: TextOverflow.ellipsis),
                  if (_queuedFiles.isNotEmpty) Text(l.kubeQueuedFiles(_queuedFiles.length)),
                  const SizedBox(height: 12),
                  if (_busy) const LinearProgressIndicator(),
                  if (_failure != null) Text(_failure!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                  if (preview == null) ...[
                    GlassButton(
                      key: const ValueKey('kube-import-file'),
                      label: l.kubeChooseFile,
                      icon: Icons.file_open_outlined,
                      onPressed: _busy ? null : _file,
                    ),
                    if (!AppPlatform.isMobile && _service is KubeInventoryManagementService)
                      GlassButton(
                        key: const ValueKey('kube-import-scan'),
                        label: l.kubeScanFolder,
                        icon: Icons.folder_open,
                        onPressed: _busy ? null : _scan,
                      ),
                    if (_queuedFiles.isNotEmpty)
                      GlassButton(label: l.kubeNextFile, onPressed: _busy ? null : _nextFile),
                    const SizedBox(height: 12),
                    TextField(
                      key: const ValueKey('kube-import-yaml'),
                      controller: _yaml,
                      enabled: !_busy,
                      minLines: 6,
                      maxLines: 12,
                      maxLength: 4 * 1024 * 1024,
                      autocorrect: false,
                      enableSuggestions: false,
                      enableIMEPersonalizedLearning: false,
                      decoration: InputDecoration(labelText: l.kubeYaml, counterText: ''),
                    ),
                  ] else ...[
                    if (_expired) Text(l.kubeExpired),
                    Text(l.kubeContextLimit),
                    for (final entry in preview.entries)
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 8),
                        child: ContentSurface(
                          child: Padding(
                            padding: const EdgeInsets.all(8),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                CheckboxListTile(
                                  key: ValueKey('kube-context-${entry.index}'),
                                  contentPadding: EdgeInsets.zero,
                                  title: Text(entry.name),
                                  subtitle: entry.server == null ? null : Text(entry.server!),
                                  value: _selected.contains(entry.index),
                                  onChanged: _busy || _expired || entry.error != null
                                      ? null
                                      : (value) => setState(() {
                                          value == true ? _selected.add(entry.index) : _selected.remove(entry.index);
                                          _filesApproved = false;
                                        }),
                                ),
                                if (entry.error != null) Text(l.kubeImportInvalid),
                                if (entry.error == null) ...[
                                  Text('${l.kubeNamespace}: ${entry.namespace}'),
                                  Text(
                                    '${l.kubeAuthentication}: ${_authenticationLabel(l, entry.authentication)}'
                                    '${entry.userName == null ? '' : ' · ${entry.userName}'}',
                                  ),
                                  if (entry.authentication == 'exec') Text(l.kubeExecHelp),
                                  SwitchListTile.adaptive(
                                    key: ValueKey('kube-production-${entry.index}'),
                                    contentPadding: EdgeInsets.zero,
                                    title: Text(l.kubeProduction),
                                    value: !_nonProduction.contains(entry.index),
                                    onChanged: _busy || _expired
                                        ? null
                                        : (value) => setState(() {
                                            value
                                                ? _nonProduction.remove(entry.index)
                                                : _nonProduction.add(entry.index);
                                          }),
                                  ),
                                  if (entry.insecureTls && !_nonProduction.contains(entry.index))
                                    Text(l.kubeTlsRequired),
                                  if (entry.requiresRoute) ...[
                                    Text(l.kubeRouteRequired),
                                    DropdownButtonFormField<String>(
                                      key: ValueKey('kube-route-${entry.index}'),
                                      isExpanded: true,
                                      decoration: InputDecoration(labelText: l.kubeRoute),
                                      items: hosts
                                          .map(
                                            (host) => DropdownMenuItem(
                                              value: host.id.value,
                                              child: Text(host.name, overflow: TextOverflow.ellipsis),
                                            ),
                                          )
                                          .toList(),
                                      onChanged: _busy || _expired
                                          ? null
                                          : (value) => setState(() {
                                              if (value == null) {
                                                _sshRoutes.remove(entry.index);
                                              } else {
                                                _sshRoutes[entry.index] = value;
                                              }
                                            }),
                                    ),
                                  ],
                                ],
                              ],
                            ),
                          ),
                        ),
                      ),
                    if (_files.isNotEmpty) ...[
                      for (final path in _files) Text(path),
                      CheckboxListTile(
                        key: const ValueKey('kube-import-approve-files'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.kubeApproveFiles),
                        value: _filesApproved,
                        onChanged: _busy || _expired ? null : (value) => setState(() => _filesApproved = value == true),
                      ),
                    ],
                    TextButton(onPressed: _busy ? null : _reset, child: Text(l.kubeChooseAgain)),
                  ],
                ],
              ),
            ),
          ),
          primaryAction: GlassButton.prominent(
            key: const ValueKey('kube-import-submit'),
            label: preview == null ? l.kubePreview : l.kubeSaveContexts,
            busy: _busy,
            onPressed: preview == null ? (_busy ? null : _paste) : (_canSave ? _save : null),
          ),
          secondaryActions: [
            GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, _savedAny)),
          ],
        ),
      ),
    );
  }
}

class _FileSelectionCancelled implements Exception {
  const _FileSelectionCancelled();
}
