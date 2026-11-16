import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_section_adapter.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Only the original chooser route is dismissed. A cached branch may survive
/// ordinary navigation, but its dialog must not cover the next section or a
/// replacement profile. There is no subscription owner, timer or backend read.
class JobContextRouteGuard extends ConsumerWidget {
  const JobContextRouteGuard({super.key, required this.focus, required this.child, this.isCurrent});
  final JobContextFocus focus;
  final Widget child;
  final bool Function()? isCurrent;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final session = ref.watch(monitoringSessionProvider);
    return ValueListenableBuilder<RouteInformation>(
      valueListenable: ref.watch(routerProvider).routeInformationProvider,
      builder: (context, route, _) {
        if (identical(session?.correlationScope, focus.scope) &&
            route.uri.path == jobContextSectionRoute(focus.selection) &&
            (isCurrent?.call() ?? true)) {
          return child;
        }
        final ownRoute = ModalRoute.of(context);
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (context.mounted && ownRoute != null && ownRoute.isActive) Navigator.of(context).removeRoute(ownRoute);
        });
        return const SizedBox.shrink();
      },
    );
  }
}
