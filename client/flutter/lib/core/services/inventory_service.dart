import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Hosts, groups, jump profiles, credentials and known hosts (decrypted
/// working set held by app-core). All `watch*` streams emit the current
/// list first. Writes go to local storage + outbox first (offline-first).
abstract interface class InventoryService {
  // Hosts
  Stream<List<Host>> watchHosts();

  /// Insert or update (by id). Keeps `credentialId` exactly as given.
  Future<Host> saveHost(Host host);

  /// Insert or update [host] together with its authentication in one atomic
  /// step (host editor "Authentication" section): creates, updates or
  /// deletes the host's inline Password credential + Secret, finds or
  /// creates agent credentials, links shared credentials, and sets
  /// `credentialId` / auth metadata accordingly. Returns the stored host.
  Future<Host> saveHostWithAuth(Host host, HostAuth auth);

  Future<void> deleteHost(ObjectId id);

  // Groups
  Stream<List<Group>> watchGroups();

  Future<Group> saveGroup(Group group);

  /// Children and hosts move to the deleted group's parent.
  Future<void> deleteGroup(ObjectId id);

  // Jump profiles
  Stream<List<JumpProfile>> watchJumpProfiles();

  Future<JumpProfile> saveJumpProfile(JumpProfile profile);

  Future<void> deleteJumpProfile(ObjectId id);

  // Credentials (metadata only; secret material stays in core)
  Stream<List<Credential>> watchCredentials();

  Future<Credential> createPasswordCredential({required String name, required SecretText password, String? username});

  /// Ed25519 (default) or RSA 3072/4096 (§7.1). Optional passphrase protects
  /// the generated key; [rememberPassphrase] stores it as a separate Secret.
  Future<Credential> generateKeyCredential({
    required String name,
    required KeyAlgorithm algorithm,
    String? comment,
    SecretText? passphrase,
    bool rememberPassphrase = false,
  });

  /// Parses a pasted OpenSSH/PEM private key without importing it.
  Future<KeyInspection> inspectPrivateKey(SecretText privateKey);

  /// Imports an OpenSSH key (optionally with a certificate). An encrypted
  /// key keeps its own passphrase protection (§7.6).
  Future<Credential> importKeyCredential({
    required String name,
    required SecretText privateKey,
    String? username,
    SecretText? passphrase,
    bool rememberPassphrase = false,
    String? certificate,
  });

  /// OS SSH agent or external agent socket / named pipe.
  Future<Credential> createAgentCredential({
    required String name,
    required CredentialKind kind,
    String? username,
    String? agentPath,
  });

  /// "Remember SSH key passphrase" for an existing encrypted key: stored as
  /// a separate Secret (CLIENT_SPEC §7.6); the key keeps its own protection.
  Future<Credential> rememberKeyPassphrase(ObjectId credentialId, SecretText passphrase);

  /// Deletes the remembered passphrase Secret of a key credential.
  Future<Credential> forgetKeyPassphrase(ObjectId credentialId);

  /// Rename / username change (no secret material involved).
  Future<Credential> updateCredential(Credential credential);

  Future<void> deleteCredential(ObjectId id);

  /// Explicit user action only ("Reveal" / "Copy password").
  Future<SecretText> revealCredentialSecret(ObjectId credentialId);

  // Connection planning (read-only previews of app-core's planner)
  Future<EffectiveHostConfig> resolveEffective(Host host);

  Future<EffectiveGroupDefaults> resolveGroupDefaults(ObjectId groupId);

  // Known hosts
  Stream<List<KnownHost>> watchKnownHosts();

  Future<void> deleteKnownHost(ObjectId id);
}
