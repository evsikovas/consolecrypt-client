import 'package:consolecrypt/monitoring/monitoring_grafana_input.dart';

/// The exact original stays in bounded memory. EditableText only receives the
/// small editor/preview projection, never a dashboard-sized paragraph.
final class GrafanaInputBuffer {
  const GrafanaInputBuffer._(this.original, this.display, this.previewOnly);
  static const editableLimit = 16 * 1024, previewLimit = 8 * 1024;
  static const empty = GrafanaInputBuffer._('', '', false);
  final String original, display;
  final bool previewOnly;
  factory GrafanaInputBuffer.accept(String text) {
    // UTF-8's exact 2MiB budget is independently enforced by the existing file
    // reader and isolated parser. No clipboard input can retain more code units.
    if (text.length > grafanaInputByteLimit) throw const GrafanaInputException(GrafanaInputFailure.tooLarge);
    if (text.length <= editableLimit) return GrafanaInputBuffer._(text, text, false);
    var end = previewLimit;
    if (text.codeUnitAt(end - 1) >= 0xd800 &&
        text.codeUnitAt(end - 1) <= 0xdbff &&
        text.codeUnitAt(end) >= 0xdc00 &&
        text.codeUnitAt(end) <= 0xdfff) {
      end--;
    }
    return GrafanaInputBuffer._(text, text.substring(0, end), true);
  }
}
