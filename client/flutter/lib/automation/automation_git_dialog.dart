import 'dart:async';

import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationGitDialog extends ConsumerStatefulWidget {
  const AutomationGitDialog({super.key, required this.scope, required this.service, required this.git});
  final AutomationScope scope;
  final AutomationService service;
  final AutomationGitService git;
  @override
  ConsumerState<AutomationGitDialog> createState() => _AutomationGitDialogState();
}

class _AutomationGitDialogState extends ConsumerState<AutomationGitDialog> {
  final name = TextEditingController(),
      url = TextEditingController(),
      reference = TextEditingController(text: 'main'),
      ca = TextEditingController();
  String? id;
  bool customCa = false, busy = false, polling = false, completed = false, scopeValid = true;
  Object? error;
  Timer? timer;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  bool get plainHttp => Uri.tryParse(url.text.trim())?.scheme == 'http';
  @override
  void initState() {
    super.initState();
    ref.listenManual(
      automationScopeProvider,
      (_, next) => scopeValid = identical(next, widget.scope),
      fireImmediately: true,
    );
  }

  @override
  void dispose() {
    timer?.cancel();
    if (!completed && scopeValid && id != null) {
      unawaited(widget.git.cancelGit(id!).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    }
    for (final controller in [name, url, reference, ca]) {
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> start() async {
    if (busy || id != null || !current) return;
    if (name.text.trim().isEmpty || url.text.trim().isEmpty || reference.text.trim().isEmpty) {
      setState(() => error = FormatException(context.l10n.automationGitFields));
      return;
    }
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final created = await widget.git.startGit(
        name: name.text.trim(),
        runnerId: '',
        url: url.text.trim(),
        reference: reference.text.trim(),
        caPem: customCa && !plainHttp ? ca.text : null,
      );
      if (!current) {
        if (scopeValid) {
          unawaited(widget.git.cancelGit(created.id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
        }
        return;
      }
      setState(() => id = created.id);
      ref.invalidate(automationProjectsProvider);
      timer = Timer.periodic(const Duration(seconds: 1), (_) => poll());
      await poll();
    } catch (e) {
      if (current) {
        setState(() {
          error = e;
          busy = false;
        });
      }
    }
  }

  Future<void> poll() async {
    if (polling || id == null || !current) return;
    polling = true;
    try {
      final catalog = await widget.service.projects();
      if (!current) return;
      final project = catalog.projects.where((project) => project.id == id).firstOrNull;
      if (project == null) {
        timer?.cancel();
        setState(() {
          busy = false;
          error = FormatException(context.l10n.automationGitFailed);
        });
        return;
      }
      if (project.importState == 'ready' && !project.readOnly) {
        completed = true;
        timer?.cancel();
        ref.invalidate(automationProjectsProvider);
        if (mounted) {
          Navigator.pop(context, project);
        }
      } else if (project.importState != 'importing') {
        timer?.cancel();
        ref.invalidate(automationProjectsProvider);
        setState(() {
          busy = false;
          error = FormatException(context.l10n.automationGitFailed);
        });
      }
    } catch (e) {
      if (current) {
        setState(() => error = e);
      }
    } finally {
      polling = false;
    }
  }

  Widget field(String label, TextEditingController controller, String key, {bool multiline = false}) => Padding(
    padding: const EdgeInsets.only(bottom: 12),
    child: TextField(
      key: ValueKey(key),
      controller: controller,
      onChanged: identical(controller, url) ? (_) => setState(() {}) : null,
      enabled: !busy && id == null,
      minLines: multiline ? 3 : 1,
      maxLines: multiline ? 8 : 1,
      decoration: InputDecoration(labelText: label),
      autocorrect: false,
      enableSuggestions: false,
    ),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      title: Text(l.automationGitImport),
      content: SizedBox(
        width: 680,
        height: MediaQuery.sizeOf(context).height * .65,
        child: ListView(
          children: [
            Text(l.automationGitHint),
            const SizedBox(height: 12),
            field(l.automationGitName, name, 'automation-git-name'),
            field(l.automationGitUrl, url, 'automation-git-url'),
            field(l.automationGitReference, reference, 'automation-git-reference'),
            const SizedBox(height: 12),
            if (plainHttp) Text(l.automationGitHttpHint),
            if (!plainHttp)
              CheckboxListTile(
                value: customCa,
                title: Text(l.automationGitCustomCa),
                onChanged: busy || id != null ? null : (value) => setState(() => customCa = value ?? false),
              ),
            if (!plainHttp && customCa) field(l.automationGitCa, ca, 'automation-git-ca', multiline: true),
            if (busy) ...[const LinearProgressIndicator(), Text(l.automationGitReceiving)],
            if (error != null)
              Text(
                error is AppException &&
                        (error as AppException).args['field'] == 'automation_git' &&
                        (error as AppException).args['rule'] == 'git_not_installed'
                    ? l.automationGitUnavailable
                    : error is FormatException
                    ? (error as FormatException).message
                    : errorMessage(l, error!),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
        if (id == null)
          FilledButton(
            key: const ValueKey('automation-git-start'),
            onPressed: busy ? null : start,
            child: Text(l.automationGitImport),
          ),
      ],
    );
  }
}
