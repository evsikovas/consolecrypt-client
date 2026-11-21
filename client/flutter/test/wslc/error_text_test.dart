import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('bridge errors display the reason and original CLI diagnostic', () {
    const error = AppError(
      code: 'runtime',
      message: 'WSLc завершился с ошибкой',
      operation: 'list_containers',
      diagnostics: 'ERROR_FILE_NOT_FOUND',
    );
    final text = errorText(error);
    expect(text, contains('WSLc завершился с ошибкой'));
    expect(text, contains('ERROR_FILE_NOT_FOUND'));
    expect(text, isNot(contains('Instance of')));
  });
}
