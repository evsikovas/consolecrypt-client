import 'package:consolecrypt/sharing/sharing_models.dart';

/// Metadata-only review. The native layer resolves credentials again at
/// publication and rejects the review when its revision stamp has changed.
final class DbSharingPreview {
  const DbSharingPreview({required this.previewJson, required this.snapshotStamp});
  final String previewJson, snapshotStamp;
  factory DbSharingPreview.fromJson(Map<String, dynamic> json) =>
      DbSharingPreview(previewJson: json['preview_json'] as String, snapshotStamp: json['snapshot_stamp'] as String);
  @override
  String toString() => 'DbSharingPreview(<redacted>)';
}

abstract interface class DatabaseSharingService {
  Future<DbSharingPreview> previewDbConnection(String connectionId, {bool includeCredentials = false});
  Future<SharingItem> publishDbConnection(
    String connectionId,
    List<SharingGrant> grants, {
    required String expectedStamp,
    bool includeCredentials = false,
  });

  /// [route] is the receiver's own `DbRoute`; a forwarded route's target
  /// (namespace, workload, container) always comes from the shared item.
  Future<void> copyDbConnection(String shareId, {required Map<String, dynamic> route, bool includeCredentials = false});
}
