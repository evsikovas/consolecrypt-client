import 'dart:convert';
import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Read-only import disclosure. This widget cannot approve or run a command.
class KubeImportExecDialog extends ConsumerWidget {
  const KubeImportExecDialog({required this.inspection, required this.scope, super.key});
  final KubeImportExecInspection inspection;
  final KubeScope scope;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final current = ref.watch(kubeScopeProvider);
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(scope, next)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!context.mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    if (!identical(scope, current) || !current.unlocked) return const SizedBox.shrink();
    final l = context.l10n;
    return GlassDialog(
      title: l.kubeExecInspect,
      width: 720,
      content: ConstrainedBox(
        constraints: BoxConstraints(maxHeight: math.min(520, MediaQuery.sizeOf(context).height * .58)),
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.kubeExecImportHelp),
              const SizedBox(height: 12),
              SelectableText(inspection.command),
              if (inspection.binaryPath case final path?) SelectableText(path),
              if (inspection.binarySha256 case final hash?)
                SelectableText('SHA-256: $hash') // l10n-ignore: algorithm name and executable digest.
              else
                Text(inspection.supported ? l.kubeExecBinaryUnavailable : l.kubeExecMobile),
              const SizedBox(height: 12),
              Text(l.kubeExecArguments),
              for (var i = 0; i < inspection.arguments.length; i++)
                SelectableText('${i + 1}: ${jsonEncode(inspection.arguments[i])}'),
              const SizedBox(height: 12),
              Text(l.kubeExecEnvironment),
              for (final entry in inspection.environment.entries)
                SelectableText('${entry.key}=${jsonEncode(entry.value)}'),
            ],
          ),
        ),
      ),
      primaryAction: GlassButton.prominent(label: l.commonContinue, onPressed: () => closeDialog(context, true)),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}
