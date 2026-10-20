import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeExportDialog extends ConsumerStatefulWidget {
  const KubeExportDialog({required this.connection, required this.service, super.key});
  final KubeConnection connection;
  final KubeInventoryManagementService service;
  @override
  ConsumerState<KubeExportDialog> createState() => _KubeExportDialogState();
}

class _KubeExportDialogState extends ConsumerState<KubeExportDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  bool _includeCredentials = false, _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  Future<void> _save() async {
    if (!_current || _busy) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    String? path;
    try {
      final files = ref.read(fileDialogServiceProvider);
      path = await files.chooseSaveFile(
        suggestedName: '${widget.connection.name.replaceAll(RegExp(r'[^a-zA-Z0-9._-]'), '_')}.yaml',
        extensions: ['yaml', 'yml'],
      );
      if (!_current || path == null) return;
      await widget.service.exportContext(widget.connection.id, path, includeCredentials: _includeCredentials);
      if (!_current) return;
      final saved = await files.finishSaveFile(path);
      if (mounted && _current && saved) {
        showSnack(context, context.l10n.kubeExportSaved);
        closeDialog<void>(context);
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      // Mobile document export uses a private staging file. The selected
      // document is managed by the OS and is never deleted here.
      if (path != null && AppPlatform.isMobile) {
        try {
          await File(path).delete();
        } catch (_) {}
      }
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      title: l.kubeExport,
      content: !_current
          ? const SizedBox.shrink()
          : Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(widget.connection.name),
                Text(l.kubeExportHelp),
                CheckboxListTile(
                  key: const ValueKey('kube-export-credentials'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l.kubeIncludeCredentials),
                  value: _includeCredentials,
                  onChanged: _busy ? null : (v) => setState(() => _includeCredentials = v == true),
                ),
                if (_failed) Text(l.kubeRequestFailed),
                if (_busy) const LinearProgressIndicator(),
              ],
            ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('kube-export-save'),
        label: l.commonSave,
        onPressed: _busy ? null : _save,
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
      ],
    );
  }
}
