import 'package:consolecrypt/monitoring/monitoring_grafana_url.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

typedef GrafanaBrowserLaunch = Future<bool> Function(Uri uri);

/// Thin existing external-launcher seam, not a background service. The Screen
/// checks the original profile/route scope immediately before calling this.
final monitoringGrafanaBrowserLauncherProvider = Provider<GrafanaBrowserLaunch>(
  (ref) => (uri) async {
    final allowed = reviewGrafanaBrowserUri(uri.toString());
    if (allowed == null || allowed != uri) return false;
    return launchUrl(allowed, mode: LaunchMode.externalApplication);
  },
);
