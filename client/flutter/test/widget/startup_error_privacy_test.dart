import 'dart:math';

import 'package:consolecrypt/core/bridge/startup_error_app.dart';
import 'package:consolecrypt/core/storage/app_storage_paths.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  testWidgets('startup diagnostics do not expose a raw error payload', (tester) async {
    final marker = List.generate(32, (_) => Random.secure().nextInt(256)).join('-');
    await tester.pumpWidget(StartupErrorApp(error: StateError(marker)));
    final details = tester.widget<SelectableText>(find.byType(SelectableText)).data!;
    expect(details.contains(marker), isFalse);
    expect(details, contains('Unexpected core failure'));
  });

  for (final failure in AppStorageFailure.values) {
    testWidgets('storage startup ${failure.name} is explicit without raw OS paths', (tester) async {
      await tester.pumpWidget(StartupErrorApp(error: AppStorageException(failure)));
      final details = tester.widget<SelectableText>(find.byType(SelectableText)).data!;
      expect(details, AppStorageException(failure).message);
      expect(find.textContaining('Your data was not changed.'), findsOneWidget);
      expect(find.textContaining('Reinstall the app'), findsNothing);
    });
  }
}
