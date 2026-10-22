import 'dart:async';

import 'package:consolecrypt/app/capture_protection.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/obscurable_text_controller.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _FakeAffinity extends WindowCaptureAffinity {
  final calls = <bool>[];

  @override
  Future<bool> setExcluded({required bool excluded}) async {
    calls.add(excluded);
    return true;
  }
}

void main() {
  test('policy: never, while secrets are shown, always', () {
    for (final visible in [false, true]) {
      expect(excludeFromCapture(ScreenCaptureProtection.off, secretsVisible: visible), isFalse);
      expect(excludeFromCapture(ScreenCaptureProtection.always, secretsVisible: visible), isTrue);
      expect(excludeFromCapture(ScreenCaptureProtection.whileSecretsVisible, secretsVisible: visible), visible);
    }
    expect(const LocalSettings().screenCaptureProtection, ScreenCaptureProtection.whileSecretsVisible);
  });

  test('the setting round-trips and unknown values fall back to the default', () {
    for (final mode in ScreenCaptureProtection.values) {
      final json = localSettingsToJson(LocalSettings(screenCaptureProtection: mode));
      expect(localSettingsFromJson(json).screenCaptureProtection, mode);
    }
    expect(
      localSettingsFromJson({'screen_capture_protection': 'bogus'}).screenCaptureProtection,
      ScreenCaptureProtection.whileSecretsVisible,
    );
    expect(localSettingsFromJson({}).screenCaptureProtection, ScreenCaptureProtection.whileSecretsVisible);
  });

  test('exposure counts holds and notifies only on visible changes', () {
    final exposure = SecretExposure();
    var notified = 0;
    exposure.addListener(() => notified++);
    final a = exposure.hold();
    final b = exposure.hold();
    expect(exposure.count, 2);
    expect(exposure.visible, isTrue);
    expect(notified, 1);
    a();
    a(); // A second release is ignored.
    expect(exposure.count, 1);
    expect(notified, 1);
    b();
    expect(exposure.visible, isFalse);
    expect(notified, 2);
  });

  testWidgets('RevealedSecret holds while visible and mounted', (tester) async {
    final exposure = SecretExposure();
    var visible = false;
    var mounted = true;
    late StateSetter update;
    await tester.pumpWidget(
      StatefulBuilder(
        builder: (context, setState) {
          update = setState;
          return Column(
            textDirection: TextDirection.ltr,
            children: [
              if (mounted) RevealedSecret(visible: visible, exposure: exposure, child: const SizedBox()),
              RevealedSecret(exposure: exposure, visible: false, child: const SizedBox()),
            ],
          );
        },
      ),
    );
    expect(exposure.count, 0);
    update(() => visible = true);
    await tester.pump();
    expect(exposure.count, 1);
    update(() => visible = false);
    await tester.pump();
    expect(exposure.count, 0);
    update(() => visible = true);
    await tester.pump();
    update(() => mounted = false);
    await tester.pump();
    expect(exposure.count, 0);
  });

  testWidgets('RevealedSecretText follows text and bullet drawing', (tester) async {
    final exposure = SecretExposure.instance;
    final controller = ObscurableTextController();
    addTearDown(controller.dispose);
    await tester.pumpWidget(RevealedSecretText(controller: controller, child: const SizedBox()));
    expect(exposure.visible, isFalse);
    controller.text = 'word word';
    await tester.pump();
    expect(exposure.visible, isFalse, reason: 'still drawn as bullets');
    controller.obscured = false;
    await tester.pump();
    expect(exposure.visible, isTrue);
    controller.text = '';
    await tester.pump();
    expect(exposure.visible, isFalse);
    controller.text = 'again';
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
    expect(exposure.visible, isFalse);
  });

  testWidgets('scope applies the policy as secrets and the setting change', (tester) async {
    final affinity = _FakeAffinity();
    final exposure = SecretExposure();
    final settings = StreamController<LocalSettings>();
    addTearDown(settings.close);
    var visible = false;
    late StateSetter update;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          windowCaptureAffinityProvider.overrideWithValue(affinity),
          localSettingsProvider.overrideWith((ref) => settings.stream),
        ],
        child: CaptureProtectionScope(
          exposure: exposure,
          child: StatefulBuilder(
            builder: (context, setState) {
              update = setState;
              return RevealedSecret(visible: visible, exposure: exposure, child: const SizedBox());
            },
          ),
        ),
      ),
    );
    // Default (settings still loading): only while secrets are shown.
    expect(affinity.calls, [false]);
    update(() => visible = true);
    await tester.pump();
    expect(affinity.calls, [false, true]);
    settings.add(const LocalSettings(screenCaptureProtection: ScreenCaptureProtection.off));
    await tester.pump();
    expect(affinity.calls, [false, true, false]);
    settings.add(const LocalSettings(screenCaptureProtection: ScreenCaptureProtection.always));
    await tester.pump();
    update(() => visible = false);
    await tester.pump();
    expect(affinity.calls, [false, true, false, true], reason: 'always stays excluded without secrets');
    settings.add(const LocalSettings());
    await tester.pump();
    expect(affinity.calls.last, isFalse);
    await tester.pumpWidget(const SizedBox());
    final calls = affinity.calls.length;
    exposure.hold();
    expect(affinity.calls.length, calls, reason: 'a disposed scope stops listening');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('the scope does nothing where capture cannot be blocked', (tester) async {
    final affinity = _FakeAffinity();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [windowCaptureAffinityProvider.overrideWithValue(affinity)],
        child: const CaptureProtectionScope(child: RevealedSecret(child: SizedBox())),
      ),
    );
    expect(affinity.calls, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
