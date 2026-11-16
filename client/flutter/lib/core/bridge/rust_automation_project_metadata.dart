import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';

typedef ProjectMetadataContextJson = Future<String> Function(String projectId);
typedef ProjectMetadataCatalogJson = Future<String> Function();
typedef ProjectMetadataPublishJson = Future<String> Function(String requestJson);

/// Bind ONLY actual generated functions after Root's FRB generation. Until then
/// RustAutomationService must not advertise this optional capability.
final class RustAutomationProjectMetadataPort implements AutomationProjectMetadataService {
  const RustAutomationProjectMetadataPort({
    required ProjectMetadataContextJson contextJson,
    required ProjectMetadataCatalogJson catalogJson,
    required ProjectMetadataPublishJson publishJson,
  }) : _context = contextJson,
       _catalog = catalogJson,
       _publish = publishJson;
  final ProjectMetadataContextJson _context;
  final ProjectMetadataCatalogJson _catalog;
  final ProjectMetadataPublishJson _publish;
  AutomationJson _decode(String raw) {
    if (utf8.encode(raw).length > 526336) throw const FormatException('automation_metadata_response_too_large');
    final value = jsonDecode(raw);
    if (value is! AutomationJson) throw const FormatException('automation_metadata_response_invalid');
    return value;
  }

  @override
  Future<AutomationJson> projectMetadataContext(String projectId) async => _decode(await _context(projectId));
  @override
  Future<AutomationJson> projectMetadataCatalog() async => _decode(await _catalog());
  @override
  Future<AutomationJson> publishProjectMetadata(AutomationJson originalFrame) async {
    final raw = jsonEncode(originalFrame);
    if (utf8.encode(raw).length > 2048) throw const FormatException('automation_metadata_request_too_large');
    return _decode(await _publish(raw));
  }
}
