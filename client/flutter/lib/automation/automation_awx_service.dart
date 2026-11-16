import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource;

enum AwxFailureReason {
  unavailable,
  configuration,
  invalidInput,
  changed,
  canceled,
  expired,
  transport,
  response,
  budget,
  forbidden,
  unauthorized,
  needsAwxInput,
  unsupportedPrompts,
  unknown,
}

/// Fixed safe reason only. Never display server bodies or error arguments.
final class AwxFailure implements Exception {
  const AwxFailure(this.reason);
  final AwxFailureReason reason;
}

/// One already-issued ORIGINAL-scope registration. The production gateway is
/// deliberately absent until Native owns the registration/join seam.
abstract interface class AwxRegisteredOperation<T> {
  Future<void> get registered;
  void startBody();
  Future<T> get result;
  Future<void> get joined;
}

/// Existing Native credential resolver + fixed AWX client only, never HTTP in
/// Dart. register returns synchronously; startBody looks up the issued ID and
/// may NOT begin/create a missing/expired registration. cancel and discard use
/// the ORIGINAL scope even after the UI epoch retires, not current-owner APIs.
abstract interface class AwxNativeGateway {
  Future<AwxNativeScope> captureScope();
  Future<AwxCatalog> catalog(AwxNativeScope original);
  Future<AwxCatalog> saveConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required int? expectedRevision,
    required MonitoringSource configuration,
  });
  Future<AwxCatalog> deleteConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required AwxConnection connection,
  });
  AwxRegisteredOperation<AwxTemplatePage> registerTemplates(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int page,
    String search,
  );
  AwxRegisteredOperation<AwxPreparedReview> registerPrepare(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int templateId,
    AwxLaunchParameters parameters,
  );
  AwxRegisteredOperation<AwxLaunchOutcome> registerLaunch(
    AwxNativeScope original,
    String operationId,
    String reviewTicket,
  );
  AwxRegisteredOperation<AwxJob> registerJob(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int jobId,
  );
  AwxRegisteredOperation<AwxEventPage> registerEvents(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int jobId,
    int after,
  );
  Future<void> cancel(AwxNativeScope original, String operationId);
  Future<void> discardReview(AwxNativeScope original, String reviewTicket);
}
