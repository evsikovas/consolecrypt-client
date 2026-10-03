import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

bool rdpScopeCurrent(WidgetRef ref, RdpScope scope) => scope.unlocked && identical(scope, ref.read(rdpScopeProvider));

/// Removes this exact route on profile/lock transitions, including a parent
/// dialog covered by a second trust decision.
class RdpScopeGuard extends ConsumerWidget {
  const RdpScopeGuard({required this.scope, required this.child, super.key});
  final RdpScope scope;
  final Widget child;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    void hideIfStale() {
      if (rdpScopeCurrent(ref, scope)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!context.mounted) return;
        final route = ModalRoute.of(context);
        if (route != null && route.isActive) Navigator.of(context).removeRoute(route);
      });
    }

    ref.listen(rdpScopeProvider, (_, _) => hideIfStale());
    hideIfStale();
    return rdpScopeCurrent(ref, scope) ? child : const SizedBox.shrink();
  }
}
