import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Profiles on this installation (ADR-0106): each is one local database +
/// one vault; Local or Synced; one active at a time. Switching locks the
/// current vault (VRK zeroised) before activating the other profile.
abstract interface class ProfileService {
  /// Emits the current state first, then changes.
  Stream<ProfilesState> watchProfiles();

  ProfilesState get currentProfiles;

  /// "Use locally (no account)". The new profile becomes active with vault
  /// phase `none` → onboarding (passphrase → Recovery Kit → verification).
  Future<Profile> createLocalProfile({required String name});

  /// "Connect to a server": registers ([createAccount]) or signs in, registers
  /// this device, and activates the new synced profile. Signing into an
  /// account that already has a profile here re-activates that profile.
  Future<Profile> createSyncedProfile({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
    String? name,
  });

  Future<void> switchTo(ProfileId id);

  Future<void> rename(ProfileId id, String name);

  /// Deletes the profile's local database and keys from this device. The
  /// server copy of a synced vault is left untouched.
  Future<void> delete(ProfileId id);
}
