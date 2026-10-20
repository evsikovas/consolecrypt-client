import 'package:consolecrypt/sharing/sharing_models.dart';

/// Metadata-only review. The native layer resolves credentials again at
/// publication and rejects the review when its revision stamp has changed.
final class KubeSharingPreview {
  const KubeSharingPreview({required this.previewJson, required this.snapshotStamp});
  final String previewJson, snapshotStamp;
  factory KubeSharingPreview.fromJson(Map<String, dynamic> json) =>
      KubeSharingPreview(previewJson: json['preview_json'] as String, snapshotStamp: json['snapshot_stamp'] as String);
  @override
  String toString() => 'KubeSharingPreview(<redacted>)';
}

abstract interface class KubernetesSharingService {
  Future<KubeSharingPreview> previewKubeContext(String contextId, {bool includeCredentials = false});
  Future<SharingItem> publishKubeContext(
    String contextId,
    List<SharingGrant> grants, {
    required String expectedStamp,
    bool includeCredentials = false,
  });
  Future<void> copyKubeContext(String shareId, {required Map<String, dynamic> route, bool includeCredentials = false});
}
