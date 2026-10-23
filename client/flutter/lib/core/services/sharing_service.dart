import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

import 'package:consolecrypt/sharing/sharing_models.dart';

abstract interface class SharingService {
  Future<SharingStatus> status();
  Future<List<SharingIdentity>> discover(String email);
  Future<List<SharingItem>> list({bool refresh = false});
  Future<SharingInvitation> inspect(String id);

  /// Accept with the pin of the owner's keys ([choosePeer] / [verifyPeer]).
  Future<SharingItem> accept(String id, String verificationId);

  /// Pin a colleague device the user chose (TR-12): `chosen` on first use,
  /// `keysChanged` when known with other keys.
  Future<PeerPin> choosePeer(SharingIdentity identity, {String? email});

  /// Safety number with [identity] and this device's QR payload (TR-11).
  Future<SafetyNumber> safetyNumber(SharingIdentity identity);

  /// Verify [identity] with the typed safety number (`safety_number`) or
  /// the colleague's QR text (`qr_code`).
  Future<PeerPin> verifyPeer(SharingIdentity identity, String method, String input, {String? email});

  /// TR-18 one-time banner: unverified recipients of secrets.
  Future<List<PeerPin>> verificationBanner();
  Future<void> dismissVerificationBanner();

  /// "Only verified recipients" vault setting (TR-14).
  Future<SharingPolicy> policy();
  Future<SharingPolicy> setPolicy(SharingPolicy policy);
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false});
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> grants);
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson);
  Future<List<SharingOutboxEntry>> rotate(String id, List<SharingGrant> grants);
  Future<void> delete(String id);

  /// Protocol 1.8: leave a received item on all devices of this account.
  Future<void> leave(String id);

  /// Protocol 1.8: owner-deleted items that the server keeps restorable.
  Future<List<SharingDeletedItem>> deleted();

  /// Protocol 1.8: republish a deleted item (queued; call [flush]).
  Future<SharingItem> restore(String id);
  Future<List<SharingOutboxEntry>> outbox();
  Future<List<SharingOutboxEntry>> flush();
  Future<SharingItem> reconcile(String id, String verificationId);
  Future<List<SharingOutboxEntry>> discardPending(String mutationId);
  Future<String> previewGroup(String groupId, String childrenJson);
  Future<String> previewSecret(String credentialId, {bool passphrase = false});
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false});
  Future<SecretText> revealSecret(String shareId);
  Future<List<SharingOutboxEntry>> editSecret(String shareId, SecretText value);
  Future<Credential> copySecretCredential(String shareId);
  Future<Host> copyHost(String shareId, {String? credentialId});
  Future<Snippet> copySnippet(String shareId);
  Future<Host> refreshBoundHost(
    String hostId, {
    bool confirmEndpointChange = false,
    String? expectedAddress,
    int? expectedPort,
  });
  Future<Host> detachHost(String hostId);
}

/// Older/demo backends have no sharing capability; this never fakes a grant.
final class UnavailableSharingService implements SharingService {
  const UnavailableSharingService();
  @override
  Future<SharingStatus> status() async => const SharingStatus();
  Never _unavailable() => throw StateError('Sharing capability unavailable'); // l10n-ignore: diagnostic
  @override
  Future<List<SharingIdentity>> discover(String email) async => _unavailable();
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => const [];
  @override
  Future<SharingInvitation> inspect(String id) async => _unavailable();
  @override
  Future<SharingItem> accept(String id, String verificationId) async => _unavailable();
  @override
  Future<PeerPin> choosePeer(SharingIdentity identity, {String? email}) async => _unavailable();
  @override
  Future<SafetyNumber> safetyNumber(SharingIdentity identity) async => _unavailable();
  @override
  Future<PeerPin> verifyPeer(SharingIdentity identity, String method, String input, {String? email}) async =>
      _unavailable();
  @override
  Future<List<PeerPin>> verificationBanner() async => const [];
  @override
  Future<void> dismissVerificationBanner() async {}
  @override
  Future<SharingPolicy> policy() async => const SharingPolicy();
  @override
  Future<SharingPolicy> setPolicy(SharingPolicy policy) async => _unavailable();
  @override
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false}) async => _unavailable();
  @override
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> grants) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> rotate(String id, List<SharingGrant> grants) async => _unavailable();
  @override
  Future<void> delete(String id) async => _unavailable();
  @override
  Future<void> leave(String id) async => _unavailable();
  @override
  Future<List<SharingDeletedItem>> deleted() async => const [];
  @override
  Future<SharingItem> restore(String id) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingOutboxEntry>> flush() async => _unavailable();
  @override
  Future<SharingItem> reconcile(String id, String verificationId) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> discardPending(String mutationId) async => _unavailable();
  @override
  Future<String> previewGroup(String groupId, String childrenJson) async => _unavailable();
  @override
  Future<String> previewSecret(String credentialId, {bool passphrase = false}) async => _unavailable();
  @override
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false}) async =>
      _unavailable();
  @override
  Future<SecretText> revealSecret(String shareId) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> editSecret(String shareId, SecretText value) async => _unavailable();
  @override
  Future<Credential> copySecretCredential(String shareId) async => _unavailable();
  @override
  Future<Host> copyHost(String shareId, {String? credentialId}) async => _unavailable();
  @override
  Future<Snippet> copySnippet(String shareId) async => _unavailable();
  @override
  Future<Host> refreshBoundHost(
    String hostId, {
    bool confirmEndpointChange = false,
    String? expectedAddress,
    int? expectedPort,
  }) async => _unavailable();
  @override
  Future<Host> detachHost(String hostId) async => _unavailable();
}
