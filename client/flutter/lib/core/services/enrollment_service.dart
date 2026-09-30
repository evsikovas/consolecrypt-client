import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';

abstract interface class EnrollmentService {
  Future<void> restorePairing(String bundle, String confirmedCode);
  Future<void> reconcile(String shareId, String confirmedOwnerCode);
  Future<List<EnrollmentRequest>> processAutomatic();
  Future<List<EnrollmentGrant>> grants(String shareId);
  Future<EnrollmentGrant> createGrant(String shareId, EnrollmentGrantCreate create);
  Future<void> revokeGrant(String shareId, String grantId);
  Future<EnrollmentPairing> exportBundle(String shareId, String grantId);
  Future<EnrollmentPairing> prepareTarget(String bundle, SharingRole role);
  Future<EnrollmentPairing> inspectPairing(String bundle);
  Future<EnrollmentPairing> endorseTarget(String bundle, String confirmedCode);
  Future<EnrollmentRequest> submitTarget(String bundle, String confirmedCode);
  Future<List<EnrollmentRequest>> requests(String shareId);
  Future<List<EnrollmentRequest>> pendingRequests();
  Future<EnrollmentRequest> challenge(String shareId, String requestId);
  Future<EnrollmentRequest> respond(String shareId, String requestId);
  Future<void> accept(String shareId, String requestId, {required bool confirmedManual});
}

class UnavailableEnrollmentService implements EnrollmentService {
  const UnavailableEnrollmentService();
  Never _unavailable() => throw const AppException(
    AppErrorCode.unsupported,
    'Owner-online enrollment unavailable', // l10n-ignore: diagnostic
  ); // l10n-ignore: diagnostic
  @override
  Future<void> restorePairing(String bundle, String confirmedCode) async => _unavailable();
  @override
  Future<void> reconcile(String shareId, String confirmedOwnerCode) async => _unavailable();
  @override
  Future<List<EnrollmentRequest>> processAutomatic() async => _unavailable();
  @override
  Future<List<EnrollmentGrant>> grants(String shareId) async => _unavailable();
  @override
  Future<EnrollmentGrant> createGrant(String shareId, EnrollmentGrantCreate create) async => _unavailable();
  @override
  Future<void> revokeGrant(String shareId, String grantId) async => _unavailable();
  @override
  Future<EnrollmentPairing> exportBundle(String shareId, String grantId) async => _unavailable();
  @override
  Future<EnrollmentPairing> prepareTarget(String bundle, SharingRole role) async => _unavailable();
  @override
  Future<EnrollmentPairing> inspectPairing(String bundle) async => _unavailable();
  @override
  Future<EnrollmentPairing> endorseTarget(String bundle, String confirmedCode) async => _unavailable();
  @override
  Future<EnrollmentRequest> submitTarget(String bundle, String confirmedCode) async => _unavailable();
  @override
  Future<List<EnrollmentRequest>> requests(String shareId) async => _unavailable();
  @override
  Future<List<EnrollmentRequest>> pendingRequests() async => _unavailable();
  @override
  Future<EnrollmentRequest> challenge(String shareId, String requestId) async => _unavailable();
  @override
  Future<EnrollmentRequest> respond(String shareId, String requestId) async => _unavailable();
  @override
  Future<void> accept(String shareId, String requestId, {required bool confirmedManual}) async => _unavailable();
}
