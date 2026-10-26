import 'package:consolecrypt/sharing/sharing_models.dart';

/// Metadata-only review of an RDP host card (FEAT-05); the password never
/// reaches Dart. Publication rejects a review whose stamp changed.
final class RdpSharingPreview {
  const RdpSharingPreview({required this.previewJson, required this.snapshotStamp});
  final String previewJson, snapshotStamp;
  factory RdpSharingPreview.fromJson(Map<String, dynamic> json) =>
      RdpSharingPreview(previewJson: json['preview_json'] as String, snapshotStamp: json['snapshot_stamp'] as String);
  @override
  String toString() => 'RdpSharingPreview(<redacted>)';
}

abstract interface class RdpSharingService {
  Future<RdpSharingPreview> previewRdpHost(String hostId, {bool includePassword = false, bool includeNotes = false});
  Future<SharingItem> publishRdpHost(
    String hostId,
    List<SharingGrant> grants, {
    required String expectedStamp,
    bool includePassword = false,
    bool includeNotes = false,
  });

  /// Saves a personal copy; the shared password only when [includePassword].
  Future<void> copyRdpHost(String shareId, {bool includePassword = false});
}
