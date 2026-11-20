import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A dialog and every nested picker belong to one unlock generation. Remove
/// this route itself on lock; do not accidentally pop an unrelated top route.
class AutomationScopeGuard extends ConsumerWidget {
  const AutomationScopeGuard({super.key, required this.scope, required this.builder});
  final AutomationScope scope;
  final WidgetBuilder builder;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (!identical(ref.watch(automationScopeProvider), scope)) {
      final route = ModalRoute.of(context);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted && route != null && route.isActive) Navigator.of(context).removeRoute(route);
      });
      return const SizedBox.shrink();
    }
    return builder(context);
  }
}
