import 'package:consolecrypt/automation/automation_service.dart';

/// Display-only metadata from the original prepared review. Native independently
/// verifies the agreement, original owner, exact manifest and unrenewed expiry.
final class AutomationOwnerConfirmation {
  const AutomationOwnerConfirmation._(this.reviewId, this.manifestDigest, this.jobId, this.issuedAt, this.expiresAt);
  final String reviewId, manifestDigest, jobId;
  final int issuedAt, expiresAt;
  static final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
  static final _digest = RegExp(r'^[0-9a-f]{64}$');
  static const _fields = {'policy', 'kind', 'required', 'manifest_digest', 'job_id', 'issued_at', 'expires_at'};

  static AutomationOwnerConfirmation? fromReview(AutomationJson review) {
    final policy = review['policy'], raw = review['owner_confirmation'];
    if (policy is! Map || policy['production'] != true || raw is! Map ||
        raw.length != _fields.length || raw.keys.any((key) => !_fields.contains(key)) ||
        raw['policy'] != 'ownerConfirmationVaultPassphraseV1' || raw['kind'] != 'ownerConfirmation' ||
        raw['required'] != true) {
      return null;
    }
    final id = review['review_id'], job = raw['job_id'], digest = raw['manifest_digest'];
    final issued = raw['issued_at'], expiry = raw['expires_at'], reviewExpiry = review['expires_at'];
    bool uuid(Object? text) => text is String && _uuid.firstMatch(text)?.end == text.length &&
        text != '00000000-0000-0000-0000-000000000000';
    if (!uuid(id) || !uuid(job) || digest is! String || _digest.firstMatch(digest)?.end != digest.length ||
        issued is! int || expiry is! int || reviewExpiry is! int || issued < 1 ||
        expiry <= issued || expiry > reviewExpiry || reviewExpiry > 253402300799) {
      return null;
    }
    return AutomationOwnerConfirmation._(id as String, digest, job as String, issued, expiry);
  }

  bool expired(DateTime now) => now.toUtc().millisecondsSinceEpoch ~/ 1000 >= expiresAt;
}
