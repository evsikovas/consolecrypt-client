import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The SFTP browser service: `AppServices.sftpBrowser`, or a basic adapter
/// over `SftpService` while a backend does not provide one.
final sftpBrowserServiceProvider = Provider<SftpBrowserService>((ref) {
  final services = ref.watch(appServicesProvider);
  return services.sftpBrowser ?? SftpBrowserFallback(services.sftp);
});

/// Active edit sessions of every host (ADR-0108).
final editSessionsProvider = StreamProvider<List<EditSessionInfo>>(
  (ref) => ref.watch(sftpBrowserServiceProvider).watchEditSessions(),
);

/// Demo controls of the mock backend (null with the real core).
final sftpDebugControlsProvider = Provider<SftpBrowserDebugControls?>((ref) {
  if (ref.watch(developerControlsProvider) == null) return null;
  final service = ref.watch(sftpBrowserServiceProvider);
  return service is SftpBrowserDebugControls ? service as SftpBrowserDebugControls : null;
});
