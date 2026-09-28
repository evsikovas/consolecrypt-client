import 'package:consolecrypt/core/models/ids.dart';

/// Mirrors `cc_models::group::Group` — host group with inheritable defaults.
final class Group {
  const Group({
    required this.id,
    required this.name,
    required this.createdAt,
    required this.updatedAt,
    this.parentId,
    this.inheritedUsername,
    this.inheritedPort,
    this.inheritedCredentialId,
    this.inheritedJumpProfileId,
    this.tags = const [],
  });

  factory Group.create(String name, {ObjectId? parentId}) {
    final now = DateTime.now().toUtc();
    return Group(id: ObjectId.generate(), name: name, parentId: parentId, createdAt: now, updatedAt: now);
  }

  final ObjectId id;
  final String name;
  final ObjectId? parentId;
  final String? inheritedUsername;
  final int? inheritedPort;
  final ObjectId? inheritedCredentialId;
  final ObjectId? inheritedJumpProfileId;
  final List<String> tags;
  final DateTime createdAt;
  final DateTime updatedAt;

  bool get hasDefaults =>
      inheritedUsername != null ||
      inheritedPort != null ||
      inheritedCredentialId != null ||
      inheritedJumpProfileId != null;
}
