import 'package:consolecrypt/wslc/src/rust/models.dart';

String errorText(Object error) {
  if (error is AppError) {
    final diagnostic = error.diagnostics;
    return '${error.message} (${error.operation}, ${error.code})'
        '${diagnostic == null || diagnostic.isEmpty ? '' : '\n$diagnostic'}';
  }
  return error.toString();
}
