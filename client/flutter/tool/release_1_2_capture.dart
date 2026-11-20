// Real ConsoleCrypt widgets and synthetic projects for the public guide.
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../test/widget/automation_jobs_test.dart' as fixture;

void main() {
  for (final locale in [AppLocale.ru, AppLocale.en, AppLocale.es, AppLocale.zh]) {
    testWidgets('capture the Automation library in ${locale.wireName}', (tester) async {
      await fixture.captureJobGuide(tester, locale);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
