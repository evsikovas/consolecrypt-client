import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Navigation metadata for one saved SSH host. This does not open SSH, choose a
/// metric selector, install software or grant access to a metric source.
final class MonitoringHostFocus {
  MonitoringHostFocus._(this.host, this._scope, this._session, this._app);

  final Host host;
  final MonitoringScope _scope;
  final MonitoringSession _session;
  final Object _app;
  bool _retired = false;

  bool current(WidgetRef ref) {
    if (_retired || !ref.context.mounted) return false;
    final saved = ref.read(hostsProvider).value?.where((value) => value.id == host.id).firstOrNull;
    final valid =
        _scope.current &&
        identical(ref.read(monitoringScopeProvider), _scope) &&
        identical(ref.read(monitoringSessionProvider), _session) &&
        identical(ref.read(appServicesProvider), _app) &&
        saved != null &&
        !saved.isRdp &&
        saved.updatedAt.isAtSameMomentAs(host.updatedAt);
    if (!valid) _retired = true;
    return valid;
  }
}

void openHostMonitoring(WidgetRef ref, Host host) {
  if (!ref.context.mounted || host.isRdp) return;
  final scope = ref.read(monitoringScopeProvider), session = ref.read(monitoringSessionProvider);
  if (!scope.current || session == null) return;
  final focus = MonitoringHostFocus._(host, scope, session, ref.read(appServicesProvider));
  if (!focus.current(ref)) return;
  ref.read(routerProvider).go(AppRoutes.monitoring, extra: focus);
}
