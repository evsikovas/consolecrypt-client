/// Mirrors `cc_models::ValidationError`.
final class ValidationError {
  const ValidationError(this.field, this.reason);

  final String field;
  final String reason;

  @override
  String toString() => 'invalid $field: $reason';
}
