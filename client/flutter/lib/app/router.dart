import 'package:consolecrypt/account/login_screen.dart';
import 'package:consolecrypt/account/welcome_screen.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/shell.dart';
import 'package:consolecrypt/backups/backups_screen.dart';
import 'package:consolecrypt/backups/restore_screen.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/credentials/credentials_screen.dart';
import 'package:consolecrypt/devices/devices_screen.dart';
import 'package:consolecrypt/groups/groups_screen.dart';
import 'package:consolecrypt/hosts/host_editor_screen.dart';
import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:consolecrypt/hosts/known_hosts_screen.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
import 'package:consolecrypt/settings/settings_screen.dart';
import 'package:consolecrypt/sftp/sftp_screen.dart';
import 'package:consolecrypt/sharing/sharing_screen.dart';
import 'package:consolecrypt/sync/sync_screen.dart';
import 'package:consolecrypt/terminal/terminal_screen.dart';
import 'package:consolecrypt/tunnels/tunnels_screen.dart';
import 'package:consolecrypt/vault/approval_wait_screen.dart';
import 'package:consolecrypt/vault/create_vault_screen.dart';
import 'package:consolecrypt/vault/recovery_kit_screen.dart';
import 'package:consolecrypt/vault/recovery_screen.dart';
import 'package:consolecrypt/vault/unlock_screen.dart';
import 'package:consolecrypt/vault/verify_kit_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

final rootNavigatorKey = GlobalKey<NavigatorState>(debugLabel: 'root');

/// Branch order of the shell (sidebar order).
enum ShellBranch {
  hosts,
  groups,
  credentials,
  knownHosts,
  terminal,
  sftp,
  tunnels,
  devices,
  sync,
  backups,
  settings,
  sharing,
  rdp,
}

Page<void> _page(Widget child) => NoTransitionPage<void>(child: child);

GoRoute _gate(String path, Widget Function(GoRouterState state) build) =>
    GoRoute(path: path, pageBuilder: (context, state) => _page(build(state)));

StatefulShellBranch _branch(String path, Widget child, {List<RouteBase> routes = const []}) => StatefulShellBranch(
  routes: [GoRoute(path: path, pageBuilder: (context, state) => _page(child), routes: routes)],
);

final routerProvider = Provider<GoRouter>((ref) {
  final stage = ValueNotifier<AppStage>(ref.read(appStageProvider));
  ref.listen(appStageProvider, (_, next) => stage.value = next);

  final router = GoRouter(
    navigatorKey: rootNavigatorKey,
    initialLocation: AppRoutes.hosts,
    refreshListenable: stage,
    redirect: (context, state) => redirectFor(stage.value, state.matchedLocation),
    routes: [
      _gate(AppRoutes.loading, (_) => const _SplashScreen()),
      _gate(AppRoutes.welcome, (_) => const WelcomeScreen()),
      _gate(AppRoutes.login, (s) => LoginScreen(newProfile: s.uri.queryParameters['new'] == '1')),
      _gate(AppRoutes.restore, (_) => const RestoreBackupScreen()),
      _gate(AppRoutes.onboarding, (_) => const CreateVaultScreen()),
      _gate(AppRoutes.recoveryKit, (_) => const RecoveryKitScreen()),
      _gate(AppRoutes.verifyKit, (_) => const VerifyKitScreen()),
      _gate(AppRoutes.localNotice, (_) => const LocalNoticeScreen()),
      _gate(AppRoutes.unlock, (_) => const UnlockScreen()),
      _gate(AppRoutes.approval, (_) => const ApprovalWaitScreen()),
      _gate(AppRoutes.recovery, (_) => const RecoveryScreen()),
      // Compatibility links open a tool next to the default workspace.
      GoRoute(path: AppRoutes.snippets, redirect: (_, _) => '${AppRoutes.hosts}?tool=snippets'),
      GoRoute(path: AppRoutes.ai, redirect: (_, _) => '${AppRoutes.hosts}?tool=ai'),
      StatefulShellRoute.indexedStack(
        builder: (context, state, shell) => AppShell(
          navigationShell: shell,
          location: state.uri.path,
          requestedTool: state.uri.queryParameters['tool'],
        ),
        branches: [
          _branch(
            AppRoutes.hosts,
            const HostsScreen(),
            routes: [
              GoRoute(
                path: 'new',
                pageBuilder: (context, state) => _page(
                  HostEditorScreen(
                    key: ValueKey('new-host-${state.uri.queryParameters['protocol'] == 'rdp' ? 'rdp' : 'ssh'}'),
                    initialProtocol: state.uri.queryParameters['protocol'] == 'rdp'
                        ? HostProtocol.rdp
                        : HostProtocol.ssh,
                    initialGroupId: state.uri.queryParameters['group'] == null
                        ? null
                        : ObjectId(state.uri.queryParameters['group']!),
                  ),
                ),
              ),
              GoRoute(
                path: ':id',
                pageBuilder: (context, state) => _page(HostEditorScreen(hostId: ObjectId(state.pathParameters['id']!))),
              ),
            ],
          ),
          _branch(AppRoutes.groups, const GroupsScreen()),
          _branch(AppRoutes.credentials, const CredentialsScreen()),
          _branch(AppRoutes.knownHosts, const KnownHostsScreen()),
          _branch(AppRoutes.terminal, const TerminalScreen()),
          _branch(AppRoutes.sftp, const SftpScreen()),
          _branch(AppRoutes.tunnels, const TunnelsScreen()),
          _branch(AppRoutes.devices, const DevicesScreen()),
          _branch(AppRoutes.sync, const SyncScreen()),
          _branch(AppRoutes.backups, const BackupsScreen()),
          _branch(AppRoutes.settings, const SettingsScreen()),
          _branch(AppRoutes.sharing, const SharingScreen()),
          _branch(AppRoutes.rdp, const RdpScreen()),
        ],
      ),
    ],
  );
  ref.onDispose(() {
    router.dispose();
    stage.dispose();
  });
  return router;
});

class _SplashScreen extends StatelessWidget {
  const _SplashScreen();

  @override
  Widget build(BuildContext context) => const Scaffold(body: Center(child: CircularProgressIndicator()));
}
