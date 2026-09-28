import 'dart:async';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/design_gallery_screen.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Pumps [child] under a glass theme + [GlassScope] (no Riverpod).
Future<GlassBackdropBudget> pumpGlass(
  WidgetTester tester,
  Widget child, {
  Brightness brightness = Brightness.light,
  bool highContrast = false,
  TargetPlatform platform = TargetPlatform.macOS,
  GlassAppearance appearance = GlassAppearance.fallback,
  Size size = const Size(1400, 1000),
  bool scroll = false,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);
  final budget = GlassBackdropBudget();
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.build(brightness, highContrast: highContrast, platform: platform),
      builder: (context, child) => GlassScope(
        data: GlassScopeData(appearance: appearance, budget: budget),
        child: child!,
      ),
      home: Scaffold(
        body: scroll ? SingleChildScrollView(padding: const EdgeInsets.all(16), child: child) : child,
      ),
    ),
  );
  await tester.pump();
  return budget;
}

Iterable<RenderObject> _renderObjects(Finder finder) sync* {
  for (final element in finder.evaluate()) {
    final root = element.renderObject;
    if (root == null) continue;
    final stack = [root];
    while (stack.isNotEmpty) {
      final r = stack.removeLast();
      yield r;
      r.visitChildren(stack.add);
    }
  }
}

/// Enabled backdrop filters (live blur) in the whole tree.
List<RenderBackdropFilter> _liveBackdrops() => [
  for (final r in _renderObjects(find.byType(MaterialApp)))
    if (r is RenderBackdropFilter && r.enabled) r,
];

Rect _globalRect(RenderBox box) => MatrixUtils.transformRect(box.getTransformTo(null), Offset.zero & box.size);

void main() {
  group('every kit component builds', () {
    final themes = [
      (Brightness.light, false),
      (Brightness.dark, false),
      (Brightness.light, true),
      (Brightness.dark, true),
    ];
    for (final (brightness, ic) in themes) {
      for (final mode in GlassMode.values) {
        testWidgets('${brightness.name}${ic ? '-IC' : ''} · ${mode.name} · macOS', (tester) async {
          final appearance = resolveEffectiveGlass(mode: mode, mediaHighContrast: ic).appearance;
          await pumpGlass(
            tester,
            const GlassKitShowcase(),
            brightness: brightness,
            highContrast: ic,
            appearance: appearance,
            scroll: true,
          );
          expect(tester.takeException(), isNull);
          expect(find.byType(GlassVerificationCode), findsOneWidget);
          expect(find.byType(GlassTabStrip), findsOneWidget);
        }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
      }
      testWidgets('${brightness.name}${ic ? '-IC' : ''} · Windows · Reduce Motion', (tester) async {
        await pumpGlass(
          tester,
          const GlassKitShowcase(),
          brightness: brightness,
          highContrast: ic,
          platform: TargetPlatform.windows,
          appearance: GlassAppearance(increaseContrast: ic, reduceMotion: true),
          size: const Size(1024, 720),
          scroll: true,
        );
        expect(tester.takeException(), isNull);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }

    testWidgets('refractive tier without a shader stays frosted', (tester) async {
      await pumpGlass(
        tester,
        const GlassKitShowcase(),
        appearance: const GlassAppearance(tier: GlassTier.refractive),
        scroll: true,
      );
      expect(tester.takeException(), isNull);
      for (final r in _renderObjects(find.byType(MaterialApp))) {
        if (r is RenderGlassBackdrop) expect(r.isRefracting, isFalse);
      }
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('SecureSurface never blurs, refracts or animates', () {
    bool forbiddenWidget(Widget w) =>
        w is BackdropFilter ||
        w is GlassBackdrop ||
        w is ImageFiltered ||
        w is ImplicitlyAnimatedWidget ||
        w is AnimatedWidget ||
        w is ShaderMask ||
        w is GlassSurface;

    bool blurWidget(Widget w) =>
        w is BackdropFilter || w is GlassBackdrop || w is ImageFiltered || w is ShaderMask || w is GlassSurface;

    /// The material itself adds no blur/shader/animation; content inside it
    /// never blurs and kit controls do not animate there.
    void expectClean(Finder secure) {
      expect(secure, findsWidgets);
      final content = find.descendant(of: secure, matching: find.byKey(const ValueKey('secure-surface-content')));
      final own = find.descendant(of: secure, matching: find.byWidgetPredicate(forbiddenWidget)).evaluate().toSet()
        ..removeAll(find.descendant(of: content, matching: find.byWidgetPredicate(forbiddenWidget)).evaluate())
        ..removeAll(content.evaluate());
      expect(own, isEmpty, reason: 'SecureSurface structure');
      expect(find.descendant(of: secure, matching: find.byWidgetPredicate(blurWidget)), findsNothing);
      expect(find.descendant(of: secure, matching: find.byType(AnimatedScale)), findsNothing);
      for (final r in _renderObjects(secure)) {
        expect(r, isNot(isA<RenderBackdropFilter>()));
        expect(r, isNot(isA<RenderShaderMask>()));
      }
    }

    for (final appearance in const [
      GlassAppearance.fallback,
      GlassAppearance(tier: GlassTier.refractive),
      GlassAppearance(mode: GlassMode.clear),
      GlassAppearance(increaseContrast: true),
      GlassAppearance(tier: GlassTier.solid, mode: GlassMode.solid),
    ]) {
      testWidgets('inline · $appearance', (tester) async {
        await pumpGlass(
          tester,
          const Center(
            child: SecureSurface(
              child: GlassVerificationCode(groups: ['11111', '22222', '33333', '44444', '55555', '66666']),
            ),
          ),
          appearance: appearance,
        );
        expectClean(find.byType(SecureSurface));
        final box = tester.widget<DecoratedBox>(find.byKey(const ValueKey('secure-surface-fill')));
        final alpha = (box.decoration as ShapeDecoration).color!.a;
        expect(alpha, greaterThanOrEqualTo(0.96));
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }

    testWidgets('secure dialog and device approval', (tester) async {
      await pumpGlass(tester, const GlassKitShowcase(), scroll: true);
      await tester.ensureVisible(find.byKey(const ValueKey('gallery-open-secure')));
      await tester.tap(find.byKey(const ValueKey('gallery-open-secure')));
      await settle(tester);
      expect(find.text('Run on prod-db-1?'), findsOneWidget);
      expectClean(find.byType(SecureSurface).last);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(tester);

      await tester.ensureVisible(find.byKey(const ValueKey('gallery-open-approval')));
      await tester.tap(find.byKey(const ValueKey('gallery-open-approval')));
      await settle(tester);
      expect(find.byKey(const ValueKey('gallery-approval-code')), findsOneWidget);
      expectClean(find.byType(SecureSurface).last);
      final fill = tester.widget<DecoratedBox>(find.byKey(const ValueKey('secure-surface-fill')).last);
      expect((fill.decoration as ShapeDecoration).color!.a, 1.0, reason: 'approval is fully opaque');
      // Approve stays disabled until the checkbox is ticked.
      await tester.tap(find.byKey(const ValueKey('gallery-approve')));
      await tester.pump();
      expect(find.byKey(const ValueKey('gallery-approval-code')), findsOneWidget);
      // No live blur anywhere while the approval is open.
      expect(_liveBackdrops(), isEmpty);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('live backdrop budget', () {
    testWidgets('only one persistent chrome surface blurs; static chrome has no filter', (tester) async {
      final budget = await pumpGlass(
        tester,
        const Column(
          children: [
            GlassCapsule(backdrop: BackdropMode.live, child: Text('a')),
            GlassCapsule(backdrop: BackdropMode.live, child: Text('b')),
            GlassCapsule(child: Text('static')),
          ],
        ),
      );
      expect(_liveBackdrops(), hasLength(1));
      expect(budget.current.denied, 1);
      final staticSurface = find.ancestor(of: find.text('static'), matching: find.byType(GlassSurface));
      expect(_renderObjects(staticSurface).whereType<RenderBackdropFilter>(), isEmpty);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('solid tier and suppression turn live glass off without rebuilding the child', (tester) async {
      final budget = await pumpGlass(
        tester,
        const Center(
          child: GlassPanel(
            backdrop: BackdropMode.live,
            child: SizedBox(width: 200, child: TextField(key: ValueKey('glass-input'))),
          ),
        ),
      );
      expect(_liveBackdrops(), hasLength(1));
      await tester.enterText(find.byKey(const ValueKey('glass-input')), 'kept');
      final owner = Object();
      budget.suppress(owner, includeOverlays: false);
      await tester.pump();
      await tester.pump();
      expect(_liveBackdrops(), isEmpty);
      expect(find.text('kept'), findsOneWidget, reason: 'child state survives');
      budget.unsuppress(owner);
      await tester.pump();
      await tester.pump();
      expect(_liveBackdrops(), hasLength(1));
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('a second overlay falls back to the solid tier', (tester) async {
      await pumpGlass(
        tester,
        const Row(
          children: [
            GlassPanel(overlay: true, backdrop: BackdropMode.live, child: Text('menu')),
            GlassPanel(overlay: true, backdrop: BackdropMode.live, child: Text('submenu')),
          ],
        ),
      );
      expect(_liveBackdrops(), hasLength(1));
      final painters = tester
          .widgetList<CustomPaint>(find.descendant(of: find.byType(GlassSurface), matching: find.byType(CustomPaint)))
          .map((p) => p.painter)
          .whereType<GlassFillPainter>()
          .toList();
      expect(painters.map((p) => p.resolved.tint.a), containsAll([0.66, 1.0]));
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('interaction', () {
    testWidgets('buttons: tap, disabled, keyboard focus ring, Enter, 24 px hit target', (tester) async {
      var taps = 0;
      await pumpGlass(
        tester,
        Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              GlassButton(key: const ValueKey('b1'), label: 'Go', size: GlassControlSize.sm, onPressed: () => taps++),
              const GlassButton(key: ValueKey('b2'), label: 'Off', onPressed: null),
            ],
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('b1')));
      await tester.pump(const Duration(milliseconds: 200));
      expect(taps, 1);
      await tester.tap(find.byKey(const ValueKey('b2')), warnIfMissed: false);
      expect(taps, 1);
      expect(tester.getSize(find.byKey(const ValueKey('b1'))).height, greaterThanOrEqualTo(GlassSizes.minHitTarget));

      FocusManager.instance.highlightStrategy = FocusHighlightStrategy.alwaysTraditional;
      addTearDown(() => FocusManager.instance.highlightStrategy = FocusHighlightStrategy.automatic);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      final ring = tester.widget<GlassFocusRing>(
        find.descendant(of: find.byKey(const ValueKey('b1')), matching: find.byType(GlassFocusRing)),
      );
      expect(ring.visible, isTrue);
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pump(const Duration(milliseconds: 200));
      expect(taps, 2);
      final semantics = tester.getSemantics(find.byKey(const ValueKey('b1')));
      expect(semantics.flagsCollection.isButton, isTrue);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('hotspot only for interactive frosted glass (not RM / IC / solid)', (tester) async {
      Future<bool> hasLight(GlassAppearance a) async {
        await pumpGlass(
          tester,
          const Center(child: GlassCapsule(interactive: true, child: Text('x'))),
          appearance: a,
        );
        return find.byKey(const ValueKey('glass-light')).evaluate().isNotEmpty;
      }

      expect(await hasLight(GlassAppearance.fallback), isTrue);
      expect(await hasLight(const GlassAppearance(reduceMotion: true)), isFalse);
      expect(await hasLight(const GlassAppearance(increaseContrast: true)), isFalse);
      expect(await hasLight(const GlassAppearance(tier: GlassTier.solid)), isFalse);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('segmented: tap and arrow keys', (tester) async {
      var value = 'a';
      await pumpGlass(
        tester,
        StatefulBuilder(
          builder: (context, setState) => Center(
            child: GlassSegmented<String>(
              selected: value,
              onChanged: (v) => setState(() => value = v),
              segments: const [
                GlassSegment(value: 'a', label: 'A', key: ValueKey('seg-a')),
                GlassSegment(value: 'b', label: 'B', key: ValueKey('seg-b')),
                GlassSegment(value: 'c', label: 'C', key: ValueKey('seg-c')),
              ],
            ),
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('seg-b')));
      await tester.pumpAndSettle();
      expect(value, 'b');
      // Keyboard: focus the control, then ←/→ move the selection.
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.pumpAndSettle();
      expect(value, 'c');
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('menu: open, select, destructive item, Escape', (tester) async {
      String? selected;
      await pumpGlass(
        tester,
        Center(
          child: GlassMenuButton<String>(
            entries: const [
              GlassMenuItem(value: 'copy', label: 'Copy', shortcut: '⌘C', key: ValueKey('menu-copy')),
              GlassMenuDivider(),
              GlassMenuItem(value: 'delete', label: 'Delete…', destructive: true),
            ],
            onSelected: (v) => selected = v,
            builder: (context, open) => GlassButton(key: const ValueKey('open'), label: 'Menu', onPressed: open),
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      expect(find.text('Delete…'), findsOneWidget);
      expect(_liveBackdrops(), hasLength(1), reason: 'the menu is a live overlay');
      await tester.tap(find.byKey(const ValueKey('menu-copy')));
      await tester.pumpAndSettle();
      expect(selected, 'copy');
      expect(find.text('Delete…'), findsNothing);

      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.text('Delete…'), findsNothing);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('dialog: live thick glass, Enter submits, Escape cancels', (tester) async {
      var submitted = 0;
      await pumpGlass(
        tester,
        Builder(
          builder: (context) => Center(
            child: GlassButton(
              key: const ValueKey('open'),
              label: 'Open',
              onPressed: () => unawaited(
                showGlassDialog<void>(
                  context,
                  builder: (context) => GlassDialog(
                    title: 'Rename',
                    content: const GlassField(fieldKey: ValueKey('name'), autofocus: true),
                    primaryAction: GlassButton.prominent(label: 'Save', onPressed: () => Navigator.pop(context)),
                    onSubmit: () {
                      submitted++;
                      Navigator.pop(context);
                    },
                  ),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('open')));
      await tester.pumpAndSettle();
      expect(find.text('Rename'), findsOneWidget);
      expect(_liveBackdrops(), hasLength(1));
      await tester.enterText(find.byKey(const ValueKey('name')), 'web-1');
      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(submitted, 1);
      expect(find.text('Rename'), findsNothing);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('toasts: auto-dismiss after 4 s, errors stay, at most 3', (tester) async {
      await pumpGlass(
        tester,
        Builder(
          builder: (context) => Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                GlassButton(
                  key: const ValueKey('info'),
                  label: 'Info',
                  onPressed: () => GlassToast.show(context, const GlassToastRequest(message: 'Saved')),
                ),
                GlassButton(
                  key: const ValueKey('error'),
                  label: 'Error',
                  onPressed: () =>
                      GlassToast.show(context, const GlassToastRequest(message: 'Failed', tone: GlassTone.danger)),
                ),
              ],
            ),
          ),
        ),
      );
      await tester.tap(find.byKey(const ValueKey('info')));
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.text('Saved'), findsOneWidget);
      await tester.pump(const Duration(seconds: 4));
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.text('Saved'), findsNothing);

      await tester.tap(find.byKey(const ValueKey('error')));
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pump(const Duration(seconds: 10));
      expect(find.text('Failed'), findsOneWidget);
      for (var i = 0; i < 4; i++) {
        await tester.tap(find.byKey(const ValueKey('info')));
        await tester.pump(const Duration(milliseconds: 50));
      }
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.text('Saved'), findsNWidgets(3));
      expect(find.text('Failed'), findsNothing, reason: 'oldest toast dropped');
      await tester.pump(const Duration(seconds: 5));
      await tester.pump(const Duration(milliseconds: 300));
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('secret fields are refused on non-secure glass', (tester) async {
      await pumpGlass(
        tester,
        const Center(
          child: GlassPanel(child: SizedBox(width: 200, child: GlassField(secret: true))),
        ),
      );
      expect(tester.takeException(), isA<FlutterError>());
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('secret fields work on SecureSurface: obscured, reveal toggle', (tester) async {
      await pumpGlass(
        tester,
        const Center(
          child: SecureSurface(
            child: SizedBox(width: 240, child: GlassField(secret: true, fieldKey: ValueKey('secret'))),
          ),
        ),
      );
      expect(tester.takeException(), isNull);
      TextField field() => tester.widget<TextField>(find.byKey(const ValueKey('secret')));
      expect(field().obscureText, isTrue);
      expect(field().enableSuggestions, isFalse);
      expect(field().autocorrect, isFalse);
      await tester.tap(find.byTooltip('Show'));
      await tester.pump();
      expect(field().obscureText, isFalse);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('design gallery', () {
    Future<void> pumpGallery(WidgetTester tester, Size size) async {
      tester.view.physicalSize = size;
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);
      final backend = testBackend();
      addTearDown(backend.dispose);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [appServicesProvider.overrideWithValue(backend.services)],
          child: const DesignGalleryApp(),
        ),
      );
      await settle(tester);
    }

    for (final size in const [Size(1600, 1000), Size(1024, 720)]) {
      testWidgets('renders at ${size.width.toInt()} px; no persistent glass over the terminal', (tester) async {
        await pumpGallery(tester, size);
        expect(tester.takeException(), isNull);
        final terminal = find.byKey(const ValueKey('gallery-terminal'));
        await tester.ensureVisible(terminal);
        await settle(tester);
        final terminalRect = _globalRect(tester.renderObject<RenderBox>(terminal));
        expect(terminalRect.isEmpty, isFalse);
        // Persistent chrome: sidebar and toolbar glass.
        final chrome = [
          ...find.byKey(const ValueKey('glass-sidebar')).evaluate(),
          ...find.descendant(of: find.byType(GlassToolbar), matching: find.byType(GlassSurface)).evaluate(),
        ];
        expect(chrome, isNotEmpty);
        for (final element in chrome) {
          final rect = _globalRect(element.renderObject! as RenderBox);
          expect(rect.overlaps(terminalRect), isFalse, reason: 'chrome $rect over terminal $terminalRect');
        }
        // Nothing live-blurred sits on top of the terminal either.
        for (final filter in _liveBackdrops()) {
          expect(_globalRect(filter).overlaps(terminalRect), isFalse);
        }
        // No glass inside the terminal block.
        expect(find.descendant(of: terminal, matching: find.byType(GlassSurface)), findsNothing);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }

    testWidgets('Glass mode control persists the setting and drives the scope', (tester) async {
      await pumpGallery(tester, const Size(1600, 1000));
      await tester.ensureVisible(find.byKey(const ValueKey('gallery-mode-solid')));
      await tester.tap(find.byKey(const ValueKey('gallery-mode-solid')));
      await settle(tester);
      final context = tester.element(find.byType(GlassKitShowcase));
      expect(GlassScope.of(context).appearance.tier, GlassTier.solid);
      final container = ProviderScope.containerOf(context);
      expect(container.read(settingsServiceProvider).currentLocal.glassMode, GlassMode.solid);
      expect(_liveBackdrops(), isEmpty);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('opens from the app with the debug shortcut', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      await pumpApp(tester, backend);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyD);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
      await settle(tester);
      expect(find.byType(DesignGalleryScreen), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });

  group('app integration', () {
    testWidgets('native accessibility signals reach the scope and the theme', (tester) async {
      const channel = MethodChannel(AccessibilityBridge.channelName);
      tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'getSignals') return {'reduceTransparency': true};
        return null;
      });
      addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(channel, null));
      final backend = testBackend();
      addTearDown(backend.dispose);
      await pumpApp(tester, backend);
      BuildContext ctx() => tester.element(find.byType(AppCommandsScope));
      expect(GlassScope.of(ctx()).appearance.tier, GlassTier.solid);
      expect(GlassScope.of(ctx()).solidReason, GlassSolidReason.reduceTransparency);
      expect(Theme.of(ctx()).extension<GlassTokens>()!.highContrast, isFalse);

      // The runner pushes a change: Increase Contrast on, Reduce Transparency off.
      await tester.binding.defaultBinaryMessenger.handlePlatformMessage(
        AccessibilityBridge.channelName,
        const StandardMethodCodec().encodeMethodCall(
          const MethodCall('signalsChanged', {'increaseContrast': true, 'reduceTransparency': false}),
        ),
        (_) {},
      );
      await settle(tester);
      expect(GlassScope.of(ctx()).appearance.increaseContrast, isTrue);
      expect(GlassScope.of(ctx()).appearance.tier, isNot(GlassTier.solid));
      expect(Theme.of(ctx()).extension<GlassTokens>()!.highContrast, isTrue);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

    testWidgets('the Glass setting from SettingsService reaches the app scope', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await pumpApp(tester, backend);
      BuildContext ctx() => tester.element(find.byType(AppCommandsScope));
      expect(GlassScope.of(ctx()).appearance.mode, GlassMode.standard);
      final settings = backend.services.settings;
      unawaited(settings.updateLocal(settings.currentLocal.copyWith(glassMode: GlassMode.tinted)));
      await settle(tester);
      expect(GlassScope.of(ctx()).appearance.mode, GlassMode.tinted);
      unawaited(settings.updateLocal(settings.currentLocal.copyWith(glassMode: GlassMode.solid)));
      await settle(tester);
      expect(GlassScope.of(ctx()).appearance.tier, GlassTier.solid);
      expect(GlassScope.of(ctx()).solidReason, GlassSolidReason.setting);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  });
}
