import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final local = Profile(id: ProfileId.generate(), name: 'Personal', kind: ProfileKind.local, createdAt: DateTime.now());
  final synced = Profile(
    id: ProfileId.generate(),
    name: 'Work',
    kind: ProfileKind.synced,
    serverUrl: Uri.parse('https://sync.example.org'),
    accountEmail: 'a@example.org',
    createdAt: DateTime.now(),
  );
  final session = AccountSession(
    serverUrl: Uri.parse('https://sync.example.org'),
    email: 'a@example.org',
    userId: 'u',
    deviceId: DeviceId.generate(),
    deviceName: 'Mac',
  );
  ProfilesState active(Profile p) => ProfilesState(profiles: [p], activeId: p.id);

  group('computeStage', () {
    test('no profile → welcome', () {
      expect(computeStage(ProfilesState.empty, AuthState.signedOut, VaultStatus.none), AppStage.welcome);
    });

    test('local profiles need no account', () {
      expect(computeStage(active(local), AuthState.signedOut, VaultStatus.none), AppStage.needsVault);
      expect(
        computeStage(active(local), AuthState.signedOut, const VaultStatus(phase: VaultPhase.locked)),
        AppStage.locked,
      );
    });

    test('synced profile without session → signedOut', () {
      expect(
        computeStage(active(synced), AuthState.signedOut, const VaultStatus(phase: VaultPhase.unlocked)),
        AppStage.signedOut,
      );
    });

    test('recovery kit must be verified before the app opens', () {
      final auth = AuthState(session: session);
      expect(
        computeStage(active(synced), auth, const VaultStatus(phase: VaultPhase.unlocked, recoveryKitConfirmed: false)),
        AppStage.recoveryKitPending,
      );
      expect(computeStage(active(synced), auth, const VaultStatus(phase: VaultPhase.unlocked)), AppStage.unlocked);
      expect(
        computeStage(active(synced), auth, const VaultStatus(phase: VaultPhase.awaitingApproval)),
        AppStage.awaitingApproval,
      );
    });
  });

  group('redirectFor', () {
    test('locked vault cannot reach app screens', () {
      expect(redirectFor(AppStage.locked, AppRoutes.hosts), AppRoutes.unlock);
      expect(redirectFor(AppStage.locked, AppRoutes.knownHosts), AppRoutes.unlock);
      expect(redirectFor(AppStage.locked, '/hosts/new'), AppRoutes.unlock);
      expect(redirectFor(AppStage.locked, AppRoutes.recovery), isNull);
      expect(redirectFor(AppStage.locked, AppRoutes.unlock), isNull);
    });

    test('onboarding cannot be skipped', () {
      expect(redirectFor(AppStage.recoveryKitPending, AppRoutes.hosts), AppRoutes.recoveryKit);
      expect(redirectFor(AppStage.recoveryKitPending, AppRoutes.verifyKit), isNull);
      expect(redirectFor(AppStage.recoveryKitPending, AppRoutes.localNotice), isNull);
      expect(redirectFor(AppStage.needsVault, AppRoutes.recoveryKit), AppRoutes.onboarding);
    });

    test('unlocked users leave gate screens', () {
      expect(redirectFor(AppStage.unlocked, AppRoutes.unlock), AppRoutes.hosts);
      expect(redirectFor(AppStage.unlocked, AppRoutes.onboarding), AppRoutes.hosts);
      expect(redirectFor(AppStage.unlocked, '/hosts/abc'), isNull);
      expect(redirectFor(AppStage.unlocked, AppRoutes.devices), isNull);
      expect(redirectFor(AppStage.unlocked, AppRoutes.knownHosts), isNull);
    });

    test('profile creation is reachable from every stage', () {
      for (final stage in AppStage.values) {
        for (final route in AppRoutes.profileCreation) {
          expect(redirectFor(stage, route), isNull, reason: '$stage $route');
        }
      }
    });

    test('welcome and signed-out stages route home', () {
      expect(redirectFor(AppStage.welcome, AppRoutes.hosts), AppRoutes.welcome);
      expect(redirectFor(AppStage.signedOut, AppRoutes.unlock), AppRoutes.login);
      expect(redirectFor(AppStage.loading, AppRoutes.hosts), AppRoutes.loading);
    });
  });
}
