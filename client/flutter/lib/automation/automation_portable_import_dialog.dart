import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/automation/automation_portable_import_controller.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Integration supplies proper RU/EN copy. No active ARB/generated change here.
final class PortableImportLabels {
  const PortableImportLabels({
    required this.title,
    required this.warning,
    required this.chooseFile,
    required this.noFile,
    required this.name,
    required this.password,
    required this.prepare,
    required this.accept,
    required this.cancel,
    required this.close,
    required this.edit,
    required this.preparing,
    required this.accepting,
    required this.previewOnly,
    required this.normalizedName,
    required this.futureId,
    required this.limitedReview,
    required this.installed,
    required this.summary,
    required this.issue,
    required this.publicationReason,
  });
  final String title,
      warning,
      chooseFile,
      noFile,
      name,
      password,
      prepare,
      accept,
      cancel,
      close,
      edit,
      preparing,
      accepting,
      previewOnly,
      normalizedName,
      futureId,
      limitedReview,
      installed;
  final String Function(int files, int bytes, int excluded) summary;
  final String Function(PortableImportIssue) issue;
  final String Function(String) publicationReason;
}

final class _RawNameBound extends TextInputFormatter {
  const _RawNameBound();
  @override
  TextEditingValue formatEditUpdate(TextEditingValue oldValue, TextEditingValue next) {
    if (next.text.length > portableImportMaxRawNameBytes ||
        utf8.encode(next.text).length > portableImportMaxRawNameBytes) {
      return oldValue;
    }
    return next; // no Dart trim/default/normalization before captured intent
  }
}

/// A secondary explicit review route. No mount/poll/build invokes prepare,
/// Native accept, project selection, file open, query, Runner or SSH execution.
/// Host owns original provider/route/service capture BEFORE all async work.
class AutomationPortableImportDialog extends ConsumerStatefulWidget {
  const AutomationPortableImportDialog({
    super.key,
    required this.controller,
    required this.originalScope,
    required this.labels,
  });
  final AutomationPortableImportController controller;
  final AutomationScope originalScope;
  final PortableImportLabels labels;
  @override
  ConsumerState<AutomationPortableImportDialog> createState() => _ImportDialogState();
}

class _ImportDialogState extends ConsumerState<AutomationPortableImportDialog> {
  final password = TextEditingController(), name = TextEditingController();
  bool _closing = false;
  @override
  void initState() {
    super.initState();
    name.text = widget.controller.requestedName;
    widget.controller.addListener(_changed);
  }

  void _changed() {
    final c = widget.controller;
    if (!c.current ||
        const {
          PortableImportPhase.expired,
          PortableImportPhase.failed,
          PortableImportPhase.cancelled,
          PortableImportPhase.installed,
          PortableImportPhase.recoveryRequired,
        }.contains(c.phase)) {
      password.clear();
    }
    if (mounted && !_closing) setState(() {});
  }

  @override
  void didUpdateWidget(covariant AutomationPortableImportDialog old) {
    super.didUpdateWidget(old);
    if (!identical(old.controller, widget.controller)) {
      old.controller.removeListener(_changed);
      old.controller.dispose();
      widget.controller.addListener(_changed);
      password.clear();
      name.text = widget.controller.requestedName;
    }
  }

  void _prepare() {
    FocusManager.instance.primaryFocus?.unfocus();
    final secret = SecretText(password.text);
    password.clear();
    unawaited(widget.controller.prepare(secret));
  }

  Future<void> _chooseFile() async {
    final controller = widget.controller;
    await controller.chooseSource();
    if (!mounted ||
        !AppPlatform.isMobile ||
        !identical(controller, widget.controller) ||
        !controller.current ||
        controller.issue != null ||
        name.text.isNotEmpty) {
      return;
    }
    final filename = controller.selectedName;
    const suffix = '.$portablePackageExtension';
    final extension = filename.toLowerCase().lastIndexOf(suffix);
    final suggestion = extension > 0 ? filename.substring(0, extension).trim() : filename;
    if (suggestion.isEmpty || suggestion.runes.length > 120) return;
    name.text = suggestion;
    controller.setRequestedName(suggestion);
  }

  void _closeOwn() {
    if (_closing) return;
    _closing = true;
    final result = widget.controller.current ? widget.controller.installed : null;
    widget.controller.cancel(notify: false);
    password.clear();
    final own = ModalRoute.of(context);
    if (own?.isActive == true) Navigator.of(context).removeRoute(own!, result);
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller, l = widget.labels;
    ref.watch(appServicesProvider);
    ref.watch(automationServiceProvider);
    ref.watch(fileDialogServiceProvider);
    ref.watch(criticalRuntimeEventsProvider);
    ref.watch(activeProfileProvider);
    final same =
        identical(ref.watch(automationScopeProvider), widget.originalScope) &&
        widget.originalScope.unlocked &&
        c.current;
    if (!same) {
      c.cancel(notify: false);
      password.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _closeOwn();
      });
      return const SizedBox.shrink();
    }
    final ready = c.ready, attempted = c.attemptedReview, result = c.installed;
    final recovery = c.phase == PortableImportPhase.recoveryRequired;
    final frozen = c.busy || recovery || result != null;
    final media = MediaQuery.of(context);
    final contentHeight = (media.size.height - media.viewInsets.bottom - 300).clamp(80.0, 520.0);
    final source = c.selectedName;
    return AlertDialog(
      title: Text(l.title),
      content: SizedBox(
        width: 650,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: contentHeight),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.warning),
                const SizedBox(height: 12),
                if (!c.available)
                  InfoBanner(
                    message: l.issue(PortableImportIssue.unavailable),
                    tone: BannerTone.danger,
                    key: const ValueKey('portable-import-unavailable'),
                  ),
                if (result == null && !recovery && ready == null) ...[
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      OutlinedButton(
                        key: const ValueKey('portable-import-choose'),
                        onPressed: frozen || !c.available ? null : () => unawaited(_chooseFile()),
                        child: Text(l.chooseFile),
                      ),
                      Text(source.isEmpty ? l.noFile : source, maxLines: 2, overflow: TextOverflow.ellipsis),
                    ],
                  ),
                  const SizedBox(height: 8),
                  TextField(
                    key: const ValueKey('portable-import-name'),
                    controller: name,
                    enabled: !frozen && c.available,
                    decoration: InputDecoration(labelText: l.name),
                    inputFormatters: const [_RawNameBound()],
                    onChanged: c.setRequestedName,
                  ),
                  const SizedBox(height: 8),
                  SecretField(
                    key: const ValueKey('portable-import-password'),
                    controller: password,
                    label: l.password,
                    enabled: !frozen && c.available,
                    onChanged: (_) => c.passwordEdited(),
                  ),
                ],
                if (c.busy) ...[
                  const SizedBox(height: 8),
                  const LinearProgressIndicator(key: ValueKey('portable-import-progress')),
                  Text(c.phase == PortableImportPhase.accepting ? l.accepting : l.preparing),
                ],
                if (c.issue != null && c.issue != PortableImportIssue.cancelled)
                  InfoBanner(
                    key: const ValueKey('portable-import-issue'),
                    message: l.issue(c.issue!),
                    tone: BannerTone.danger,
                  ),
                if (ready != null) ...[
                  const SizedBox(height: 12),
                  ContentSurface(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Text(l.previewOnly, key: const ValueKey('portable-import-preview-only')),
                        Text(
                          '${l.normalizedName}: ${ready.name}',
                          key: const ValueKey('portable-import-reviewed-name'),
                        ),
                        Text('${l.futureId}: ${ready.projectId}', key: const ValueKey('portable-import-future-id')),
                        Text(
                          l.summary(ready.summary.files, ready.summary.bytes, ready.summary.exclusions),
                          key: const ValueKey('portable-import-summary'),
                        ),
                        Text(l.limitedReview),
                        if (!ready.publicationSupported)
                          InfoBanner(message: l.publicationReason(ready.publicationReason!), tone: BannerTone.warning),
                      ],
                    ),
                  ),
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: TextButton(
                      key: const ValueKey('portable-import-edit'),
                      onPressed: c.busy ? null : c.editInputs,
                      child: Text(l.edit),
                    ),
                  ),
                ],
                if (recovery && attempted != null) ...[
                  Text('${l.normalizedName}: ${attempted.name}'),
                  Text('${l.futureId}: ${attempted.projectId}'),
                ],
                if (result != null)
                  InfoBanner(
                    key: const ValueKey('portable-import-installed'),
                    message: l.installed,
                    tone: BannerTone.success,
                  ),
              ],
            ),
          ),
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('portable-import-close'),
          onPressed: _closeOwn,
          child: Text(recovery || result != null || c.phase == PortableImportPhase.accepting ? l.close : l.cancel),
        ),
        if (!recovery && result == null)
          if (ready == null)
            FilledButton(
              key: const ValueKey('portable-import-prepare'),
              onPressed: c.canPrepare && password.text.isNotEmpty ? _prepare : null,
              child: Text(l.prepare),
            )
          else
            FilledButton(
              key: const ValueKey('portable-import-accept'),
              onPressed: c.canAccept ? () => unawaited(c.accept()) : null,
              child: Text(l.accept),
            ),
      ],
    );
  }

  @override
  void dispose() {
    _closing = true;
    widget.controller.removeListener(_changed);
    widget.controller.dispose();
    password.clear();
    name.clear();
    password.dispose();
    name.dispose();
    super.dispose();
  }
}
