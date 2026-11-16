import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Instructions only. Opening this page, copying a command or refreshing
/// discovery never enables Windows features, elevates or restarts Windows.
class AutomationWslPreparation extends ConsumerStatefulWidget {
  const AutomationWslPreparation({super.key, this.overview});
  final Widget? overview;
  @override
  ConsumerState<AutomationWslPreparation> createState() => _AutomationWslPreparationState();
}

class _AutomationWslPreparationState extends ConsumerState<AutomationWslPreparation> {
  String? copied;
  Object? error;

  Future<void> copy(String id, String command) async {
    try {
      await copyPlainWithNotice(context, ref, command, notify: false);
      if (mounted) setState(() { copied = id; error = null; });
    } catch (failure) {
      if (mounted) setState(() => error = failure);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    Widget step(String title, String description, String? command, String id) => Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(color: Theme.of(context).dividerColor),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            Text(title, style: const TextStyle(fontWeight: FontWeight.w600)),
            const SizedBox(height: 6),
            Text(description),
            if (command != null) ...[
              const SizedBox(height: 8),
              SelectableText(command, key: ValueKey('$id-command'),
                style: const TextStyle(fontFamily: 'monospace')),
              Align(alignment: Alignment.centerLeft, child: TextButton.icon(
                key: ValueKey('$id-copy'), icon: Icon(copied == id ? Icons.check : Icons.copy_outlined, size: 16),
                label: Text(copied == id ? l.copiedNotice(l.copyWhatText) : l.commonCopy),
                onPressed: () => copy(id, command),
              )),
            ],
          ]),
        ),
      ),
    );
    return GlassDialog(
      title: l.automationWslPreparationTitle,
      icon: Icons.terminal,
      width: 600,
      content: SizedBox(width: 600, child: SingleChildScrollView(child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (widget.overview case final overview?) ...[overview, const SizedBox(height: 16)],
          if (error case final failure?) ...[
            InfoBanner(tone: BannerTone.danger, message: errorMessage(l, failure)),
            const SizedBox(height: 12),
          ],
          InfoBanner(tone: BannerTone.warning, message: l.automationWslPreparationNotice),
          const SizedBox(height: 12),
          step(l.automationWslPreparationInspectTitle, l.automationWslPreparationInspect,
            'wsl.exe --list --verbose', 'wsl-inspect'), // l10n-ignore: fixed read-only Windows command
          step(l.automationWslPreparationInstallTitle, l.automationWslPreparationInstall,
            'wsl.exe --install --distribution Ubuntu-24.04', 'wsl-install'), // l10n-ignore: explicit fixed distro install command
          step(l.automationWslPreparationUserTitle, l.automationWslPreparationUser, null, 'wsl-user'),
          step(l.automationWslPreparationRunnerTitle, l.automationWslPreparationRunner, null, 'wsl-runner'),
        ],
      ))),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => Navigator.pop(context)),
    );
  }
}
