import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/src/rust/api/sharing.dart' as rs;

final class RustSharingService implements SharingService {
  RustSharingService(this._hub);
  final RustBackend _hub;
  @override
  Future<SharingStatus> status() async => SharingStatus.fromJson(decodeObject(await guard(rs.sharingStatus)));
  @override
  Future<List<SharingIdentity>> discover(String email) async {
    final j = decodeObject(await guard(() => rs.sharingDiscover(email: email)));
    return (j['devices'] as List).map((v) => SharingIdentity.fromJson(v as Map<String, dynamic>)).toList();
  }

  @override
  Future<List<SharingItem>> list({bool refresh = false}) async =>
      (jsonDecode(await guard(() => rs.sharingList(refresh: refresh))) as List)
          .map((v) => SharingItem.fromJson(v as Map<String, dynamic>))
          .toList();
  @override
  Future<SharingInvitation> inspect(String id) async =>
      SharingInvitation.fromJson(decodeObject(await guard(() => rs.sharingInspect(id: id))));
  @override
  Future<SharingItem> accept(String id, String confirmedOwnerCode) async => SharingItem.fromJson(
    decodeObject(await guard(() => rs.sharingAccept(id: id, confirmedOwnerCode: confirmedOwnerCode))),
  );
  @override
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false}) => switch (kind) {
    SharingKind.host => guard(() => rs.sharingPreviewHost(id: objectId, includeNotes: includeNotes)),
    SharingKind.snippet => guard(() => rs.sharingPreviewSnippet(id: objectId)),
    SharingKind.group ||
    SharingKind.secret => throw UnsupportedError('Sharing kind not enabled'), // l10n-ignore: diagnostic
  };
  @override
  Future<SharingItem> publish(String projectionJson, List<SharingGrant> grants) async => SharingItem.fromJson(
    decodeObject(
      await guard(
        () => rs.sharingPublish(
          projectionJson: projectionJson,
          grantsJson: jsonEncode(grants.map((v) => v.toJson()).toList()),
        ),
      ),
    ),
  );
  @override
  Future<List<SharingOutboxEntry>> edit(String id, String projectionJson) async =>
      _outbox(await guard(() => rs.sharingEdit(id: id, projectionJson: projectionJson)));
  @override
  Future<List<SharingOutboxEntry>> rotate(String id, List<SharingGrant> grants) async => _outbox(
    await guard(() => rs.sharingRotate(id: id, grantsJson: jsonEncode(grants.map((v) => v.toJson()).toList()))),
  );
  @override
  Future<void> delete(String id) => guard(() => rs.sharingDelete(id: id));
  List<SharingOutboxEntry> _outbox(String raw) => (decodeObject(raw)['entries'] as List)
      .map((v) => SharingOutboxEntry.fromJson(v as Map<String, dynamic>))
      .toList();
  @override
  Future<List<SharingOutboxEntry>> outbox() async => _outbox(await guard(rs.sharingOutbox));
  @override
  Future<List<SharingOutboxEntry>> flush() async => _outbox(await guard(rs.sharingFlush));
  @override
  Future<SharingItem> reconcile(String id, String confirmedOwnerCode) async => SharingItem.fromJson(
    decodeObject(await guard(() => rs.sharingReconcile(id: id, confirmedOwnerCode: confirmedOwnerCode))));
  @override
  Future<List<SharingOutboxEntry>> discardPending(String mutationId) async =>
    _outbox(await guard(() => rs.sharingDiscardPending(id: mutationId)));
  @override
  Future<String> previewGroup(String groupId, String childrenJson) =>
    guard(() => rs.sharingPreviewGroup(id: groupId, childrenJson: childrenJson));
  @override
  Future<String> previewSecret(String credentialId, {bool passphrase = false}) =>
    guard(() => rs.sharingPreviewSecret(id: credentialId, passphrase: passphrase));
  @override
  Future<SharingItem> publishSecret(String credentialId, List<SharingGrant> grants, {bool passphrase = false}) async =>
    SharingItem.fromJson(decodeObject(await guard(() => rs.sharingPublishSecret(id: credentialId,
      passphrase: passphrase, grantsJson: jsonEncode(grants.map((g) => g.toJson()).toList())))));
  @override
  Future<SecretText> revealSecret(String shareId) async {
    final bytes = await guard(() => rs.sharingRevealSecret(id: shareId));
    try { return SecretText.fromBytes(bytes); } finally { bytes.fillRange(0, bytes.length, 0); }
  }
  @override
  Future<List<SharingOutboxEntry>> editSecret(String shareId, SecretText value) async =>
    withSecret(value, (bytes) async => _outbox(await guard(() => rs.sharingEditSecret(id: shareId, value: bytes))));
  @override
  Future<Credential> copySecretCredential(String shareId) async {
    final credential = credentialFromJson(decodeObject(await guard(() => rs.sharingCopySecretCredential(id: shareId))));
    await _hub.reload({'credential'});
    return credential;
  }
  @override
  Future<Host> copyHost(String shareId, {String? credentialId}) async {
    final host = hostFromJson(decodeObject(await guard(() => rs.sharingCopyHost(id: shareId, credentialId: credentialId))));
    await _hub.reload({'host'});
    return host;
  }
  @override
  Future<Snippet> copySnippet(String shareId) async {
    final snippet = snippetFromJson(decodeObject(await guard(() => rs.sharingCopySnippet(id: shareId))));
    await _hub.reload({'snippet'});
    return snippet;
  }
  @override
  Future<Host> refreshBoundHost(String hostId, {bool confirmEndpointChange = false, String? expectedAddress, int? expectedPort}) async {
    final host = hostFromJson(decodeObject(await guard(() => expectedAddress != null && expectedPort != null
      ? rs.sharingRefreshBoundHostExpected(id: hostId, address: expectedAddress, port: expectedPort)
      : rs.sharingRefreshBoundHost(id: hostId, confirmEndpointChange: confirmEndpointChange))));
    await _hub.reload({'host'});
    return host;
  }
  @override
  Future<Host> detachHost(String hostId) async {
    final host = hostFromJson(decodeObject(await guard(() => rs.sharingDetachHost(id: hostId))));
    await _hub.reload({'host'});
    return host;
  }
}
