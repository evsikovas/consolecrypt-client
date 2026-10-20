import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeContextTerminalDialog extends ConsumerStatefulWidget {
  const KubeContextTerminalDialog({required this.sessionId, required this.namespace, required this.service, super.key});
  final String sessionId, namespace;
  final KubeContextTerminalService service;
  @override
  ConsumerState<KubeContextTerminalDialog> createState() => _KubeContextTerminalDialogState();
}

class _KubeContextTerminalDialogState extends ConsumerState<KubeContextTerminalDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubeContextTerminalProfiles? _profiles;
  String? _profile;
  bool _busy = true, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    try {
      final profiles = await widget.service.contextTerminalProfiles();
      if (_current) {
        setState(() {
          _profiles = profiles;
          _profile = profiles.profiles.firstOrNull?.id;
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _open() async {
    if (!_current || _profile == null || _busy) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final id = await widget.service.openContextTerminal(widget.sessionId, widget.namespace, _profile!);
      if (mounted && _current) closeDialog(context, id);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
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
      title: l.kubeContextTerminal,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l.kubeContextTerminalHelp),
            if (_busy) const LinearProgressIndicator(),
            if (_failed) Text(l.kubeTerminalFailed),
            if (_profiles != null)
              DropdownButtonFormField<String>(
                initialValue: _profile,
                isExpanded: true,
                items: _profiles!.profiles.map((p) => DropdownMenuItem(value: p.id, child: Text(p.label))).toList(),
                onChanged: _busy ? null : (v) => setState(() => _profile = v),
              ),
          ],
        ),
      ),
      primaryAction: GlassButton.prominent(
        label: l.commonContinue,
        onPressed: _busy || _profile == null ? null : _open,
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<String>(context)),
      ],
    );
  }
}
