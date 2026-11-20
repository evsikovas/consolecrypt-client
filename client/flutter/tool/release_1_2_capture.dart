// Real ConsoleCrypt widgets and synthetic projects for the public guide.
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../test/helpers/automation_ui_proof.dart';
import '../test/widget/automation_portable_menu_test.dart' as fixture;

void main() {
  for (final locale in [AppLocale.ru, AppLocale.en, AppLocale.es, AppLocale.zh]) {
    testWidgets('capture the Automation library in ${locale.wireName}', (tester) async {
      await fixture.open(tester, locale: locale, width: 1440);
      expect(tester.takeException(), isNull);
      await captureAutomationWorkspace(tester, 'automation-library-${locale.wireName}');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
