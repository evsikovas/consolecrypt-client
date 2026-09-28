import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Regression: the language switcher (top bar) must never overlap the gate
/// card, including narrow windows and long (Russian) labels.
void main() {
  for (final size in const [Size(800, 640), Size(1024, 720), Size(1600, 1000)]) {
    for (final locale in const [AppLocale.en, AppLocale.ru]) {
      testWidgets('welcome top bar does not overlap the card at ${size.width.toInt()} px (${locale.wireName})', (
        tester,
      ) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        await pumpApp(tester, backend, locale: locale, size: size);

        final switcher = tester.getRect(find.byKey(const ValueKey('language-menu')));
        final card = tester.getRect(find.byKey(const ValueKey('gate-card')));
        expect(switcher.overlaps(card), isFalse, reason: 'switcher $switcher overlaps card $card');
        expect(switcher.right, lessThanOrEqualTo(size.width), reason: 'switcher inside the window');
        expect(tester.takeException(), isNull);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }
  }
}
