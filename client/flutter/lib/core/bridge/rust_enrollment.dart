import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/src/rust/api/enrollment.dart' as rs;

final class RustEnrollmentService implements EnrollmentService {
  const RustEnrollmentService();
  @override
  Future<void> restorePairing(String bundle, String confirmedCode) async {
    await guard(() => rs.enrollmentRestorePairing(bundle: bundle, confirmedCode: confirmedCode));
  }

  @override
  Future<void> reconcile(String shareId, String confirmedOwnerCode) async {
    await guard(() => rs.enrollmentReconcile(id: shareId, confirmedOwnerCode: confirmedOwnerCode));
  }

  @override
  Future<List<EnrollmentRequest>> processAutomatic() async =>
      (jsonDecode(await guard(rs.enrollmentProcessAutomatic)) as List)
          .map((v) => EnrollmentRequest.fromJson(v as Map<String, dynamic>))
          .toList();
  @override
  Future<List<EnrollmentGrant>> grants(String shareId) async =>
      (jsonDecode(await guard(() => rs.enrollmentGrants(id: shareId))) as List)
          .map((v) => EnrollmentGrant.fromJson(v as Map<String, dynamic>))
          .toList();
  @override
  Future<EnrollmentGrant> createGrant(String shareId, EnrollmentGrantCreate create) async => EnrollmentGrant.fromJson(
    decodeObject(await guard(() => rs.enrollmentCreate(id: shareId, createJson: jsonEncode(create.toJson())))),
  );
  @override
  Future<void> revokeGrant(String shareId, String grantId) async {
    await guard(() => rs.enrollmentRevoke(id: shareId, grantId: grantId));
  }

  @override
  Future<EnrollmentPairing> exportBundle(String shareId, String grantId) async =>
      EnrollmentPairing.fromJson(decodeObject(await guard(() => rs.enrollmentExport(id: shareId, grantId: grantId))));
  @override
  Future<EnrollmentPairing> prepareTarget(String bundle, SharingRole role) async => EnrollmentPairing.fromJson(
    decodeObject(await guard(() => rs.enrollmentPrepare(bundle: bundle, editor: role == SharingRole.editor))),
  );
  @override
  Future<EnrollmentPairing> inspectPairing(String bundle) async =>
      EnrollmentPairing.fromJson(decodeObject(await guard(() => rs.enrollmentInspect(bundle: bundle))));
  @override
  Future<EnrollmentPairing> endorseTarget(String bundle, String confirmedCode) async => EnrollmentPairing.fromJson(
    decodeObject(await guard(() => rs.enrollmentEndorse(bundle: bundle, confirmedCode: confirmedCode))),
  );
  @override
  Future<EnrollmentRequest> submitTarget(String bundle, String confirmedCode) async => EnrollmentRequest.fromJson(
    decodeObject(await guard(() => rs.enrollmentSubmit(bundle: bundle, confirmedCode: confirmedCode))),
  );
  @override
  Future<List<EnrollmentRequest>> requests(String shareId) async =>
      (jsonDecode(await guard(() => rs.enrollmentRequests(id: shareId))) as List)
          .map((v) => EnrollmentRequest.fromJson(v as Map<String, dynamic>))
          .toList();
  @override
  Future<List<EnrollmentRequest>> pendingRequests() async => (jsonDecode(await guard(rs.enrollmentPending)) as List)
      .map((v) => EnrollmentRequest.fromJson(v as Map<String, dynamic>))
      .toList();
  @override
  Future<EnrollmentRequest> challenge(String shareId, String requestId) async => EnrollmentRequest.fromJson(
    decodeObject(await guard(() => rs.enrollmentChallenge(id: shareId, requestId: requestId))),
  );
  @override
  Future<EnrollmentRequest> respond(String shareId, String requestId) async => EnrollmentRequest.fromJson(
    decodeObject(await guard(() => rs.enrollmentRespond(id: shareId, requestId: requestId))),
  );
  @override
  Future<void> accept(String shareId, String requestId, {required bool confirmedManual}) async {
    await guard(() => rs.enrollmentAccept(id: shareId, requestId: requestId, confirmedManual: confirmedManual));
  }
}
