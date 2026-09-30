import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

import 'package:consolecrypt/sharing/sharing_models.dart';

abstract interface class SharingService {
  Future<SharingStatus> status();
  Future<List<SharingIdentity>> discover(String email);
  Future<List<SharingItem>> list({bool refresh = false});
  Future<SharingInvitation> inspect(String id);
  Future<SharingItem> accept(String id, String confirmedOwnerCode);
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false});
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> grants);
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson);
  Future<List<SharingOutboxEntry>> rotate(String id, List<SharingGrant> grants);
  Future<void> delete(String id);
  Future<List<SharingOutboxEntry>> outbox();
  Future<List<SharingOutboxEntry>> flush();
  Future<SharingItem> reconcile(String id, String confirmedOwnerCode);
  Future<List<SharingOutboxEntry>> discardPending(String mutationId);
  Future<String> previewGroup(String groupId, String childrenJson);
  Future<String> previewSecret(String credentialId, {bool passphrase = false});
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false});
  Future<SecretText> revealSecret(String shareId);
  Future<List<SharingOutboxEntry>> editSecret(String shareId, SecretText value);
  Future<Credential> copySecretCredential(String shareId);
  Future<Host> copyHost(String shareId, {String? credentialId});
  Future<Snippet> copySnippet(String shareId);
  Future<Host> refreshBoundHost(String hostId, {bool confirmEndpointChange = false, String? expectedAddress, int? expectedPort});
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
  Future<SharingItem> accept(String id, String confirmedOwnerCode) async => _unavailable();
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
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingOutboxEntry>> flush() async => _unavailable();
  @override
  Future<SharingItem> reconcile(String id, String confirmedOwnerCode) async => _unavailable();
  @override
  Future<List<SharingOutboxEntry>> discardPending(String mutationId) async => _unavailable();
  @override
  Future<String> previewGroup(String groupId, String childrenJson) async => _unavailable();
  @override
  Future<String> previewSecret(String credentialId, {bool passphrase = false}) async => _unavailable();
  @override
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false}) async => _unavailable();
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
  Future<Host> refreshBoundHost(String hostId, {bool confirmEndpointChange = false, String? expectedAddress, int? expectedPort}) async => _unavailable();
  @override
  Future<Host> detachHost(String hostId) async => _unavailable();
}
