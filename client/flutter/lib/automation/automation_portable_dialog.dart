import 'dart:async';

import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Root supplies existing generated l10n at the integration seam. No ARB edit
/// or product string is hardcoded by this staged prerequisite.
final class PortableDialogLabels {
  const PortableDialogLabels({
    required this.exportTitle,
    required this.importTitle,
    required this.separatePassword,
    required this.confirmPassword,
    required this.warning,
    required this.knownExcluded,
    required this.wholeProject,
    required this.chooseFiles,
    required this.sizeLimit,
    required this.contents,
    required this.export,
    required this.inspect,
    required this.cancel,
    required this.success,
    required this.failed,
    required this.publicationBlocked,
    required this.summary,
  });
  final String exportTitle,
      importTitle,
      separatePassword,
      confirmPassword,
      warning,
      knownExcluded,
      wholeProject,
      chooseFiles,
      sizeLimit,
      export,
      inspect,
      cancel,
      success,
      failed,
      publicationBlocked;
  final String Function(int files, int bytes, int excluded) summary;
  final String Function(int files, int bytes) contents;
}

/// Scope itself is the EXISTING original AutomationScope. This route removes
/// ONLY itself on lock/reopen and disposes/wipes before any late completion.
class AutomationPortableDialog extends ConsumerStatefulWidget {
  const AutomationPortableDialog({
    super.key,
    required this.controller,
    required this.originalScope,
    required this.labels,
    required this.importMode,
  });
  final AutomationPortableController controller;
  final AutomationScope originalScope;
  final PortableDialogLabels labels;
  final bool importMode;
  @override
  ConsumerState<AutomationPortableDialog> createState() => _PortableDialogState();
}

class _PortableDialogState extends ConsumerState<AutomationPortableDialog> {
  final password = TextEditingController(), confirmation = TextEditingController();
  bool chooseFiles = false;
  bool get _sameOriginalProfile => widget.originalScope.profileId == widget.controller.scope.profileId;
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_changed);
    if (!_sameOriginalProfile ||
        !widget.originalScope.unlocked ||
        !identical(ref.read(automationScopeProvider), widget.originalScope) ||
        !widget.controller.current) {
      widget.controller.cancel(notify: false);
    } else if (!widget.importMode) {
      unawaited(widget.controller.load());
    }
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    widget.controller.removeListener(_changed);
    widget.controller.dispose();
    password.clear();
    confirmation.clear();
    password.dispose();
    confirmation.dispose();
    super.dispose();
  }

  void _submit() {
    final secret = SecretText(password.text), confirm = SecretText(confirmation.text);
    password.clear();
    confirmation.clear();
    if (widget.importMode) {
      confirm.wipe();
      unawaited(widget.controller.inspect(secret));
    } else {
      unawaited(widget.controller.export(secret, confirm));
    }
  }

  @override
  Widget build(BuildContext context) {
    final controller = widget.controller, l = widget.labels;
    if (!_sameOriginalProfile ||
        !identical(ref.watch(automationScopeProvider), widget.originalScope) ||
        !widget.originalScope.unlocked ||
        !controller.current) {
      controller.cancel(notify: false);
      password.clear();
      confirmation.clear();
      final own = ModalRoute.of(context);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted && own?.isActive == true) Navigator.of(context).removeRoute(own!);
      });
      return const SizedBox.shrink();
    }
    final review = controller.review, imported = controller.imported;
    final selection = controller.selected;
    final size = review?.selectionSize(selection);
    final exportReady = selection.isNotEmpty && size?.tooLarge == false;
    return AlertDialog(
      title: Text(widget.importMode ? l.importTitle : l.exportTitle),
      content: SizedBox(
        width: 620,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 480),
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.warning),
                const SizedBox(height: 8),
                if (controller.busy) const LinearProgressIndicator(),
                if (controller.phase == PortablePhase.failed)
                  InfoBanner(message: l.failed, tone: BannerTone.danger, key: const ValueKey('portable-failure')),
                if (controller.phase == PortablePhase.exported)
                  Text(l.success, key: const ValueKey('portable-exported')),
                if (imported != null) ...[
                  Text(l.summary(imported.files, imported.bytes, imported.exclusions)),
                  Text(l.publicationBlocked, key: const ValueKey('portable-import-blocked')),
                ],
                if (size != null && !widget.importMode) ...[
                  Wrap(
                    spacing: 8,
                    runSpacing: 4,
                    children: [
                      ChoiceChip(
                        key: const ValueKey('portable-whole-project'),
                        selected: !chooseFiles,
                        label: Text(l.wholeProject),
                        onSelected: controller.busy
                            ? null
                            : (_) {
                                controller.selectAll();
                                setState(() => chooseFiles = false);
                              },
                      ),
                      ChoiceChip(
                        key: const ValueKey('portable-choose-files'),
                        selected: chooseFiles,
                        label: Text(l.chooseFiles),
                        onSelected: controller.busy ? null : (_) => setState(() => chooseFiles = true),
                      ),
                    ],
                  ),
                  Text(l.contents(size.files, size.bytes)),
                  if (size.tooLarge)
                    InfoBanner(
                      message: l.sizeLimit,
                      tone: BannerTone.danger,
                      key: const ValueKey('portable-size-limit'),
                    ),
                  if (chooseFiles) _fileList(review!.choices, selection),
                  const SizedBox(height: 12),
                ],
                SecretField(
                  key: const ValueKey('portable-password'),
                  controller: password,
                  label: l.separatePassword,
                  enabled: !controller.busy,
                ),
                if (!widget.importMode) ...[
                  const SizedBox(height: 8),
                  SecretField(
                    key: const ValueKey('portable-confirm-password'),
                    controller: confirmation,
                    label: l.confirmPassword,
                    enabled: !controller.busy,
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () {
            controller.cancel();
            Navigator.of(context).pop();
          },
          child: Text(l.cancel),
        ),
        // There is no Import/Install/Commit action until real Native publication CAS.
        FilledButton(
          key: const ValueKey('portable-submit'),
          onPressed: controller.busy || !widget.importMode && !exportReady ? null : _submit,
          child: Text(widget.importMode ? l.inspect : l.export),
        ),
      ],
    );
  }

  Widget _fileList(List<PortableChoice> choices, Set<String> selection) {
    final rows = choices.where((choice) => !choice.directory || choice.selectable).toList();
    return SizedBox(
      height: (rows.length.clamp(1, 4) * 64).toDouble(),
      child: ListView.builder(
        itemCount: rows.length,
        itemBuilder: (context, index) {
          final choice = rows[index];
          return CheckboxListTile(
            key: ValueKey('portable-path-${choice.path}'),
            value: selection.contains(choice.path),
            onChanged: widget.controller.busy || !choice.selectable
                ? null
                : (value) => widget.controller.select(choice.path, value == true),
            title: Tooltip(
              message: choice.path,
              child: Text(choice.path, maxLines: 2, overflow: TextOverflow.ellipsis),
            ),
            subtitle: choice.selectable ? null : Text(widget.labels.knownExcluded),
          );
        },
      ),
    );
  }
}
