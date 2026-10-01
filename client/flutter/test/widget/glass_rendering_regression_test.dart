import 'dart:collection';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

class _PaintCount extends CustomPainter {
  _PaintCount(this.delegate);
  final CustomPainter delegate;
  int count = 0;

  @override
  void paint(Canvas canvas, Size size) {
    count++;
    delegate.paint(canvas, size);
  }

  @override
  bool shouldRepaint(_PaintCount oldDelegate) => oldDelegate.delegate != delegate;
}

class _LiveContent extends CustomPainter {
  _LiveContent(this.tick) : super(repaint: tick);
  final ValueNotifier<int> tick;
  int count = 0;

  @override
  void paint(Canvas canvas, Size size) {
    count++;
    canvas.drawRect(Rect.fromLTWH(tick.value.toDouble(), 10, 20, 20), Paint()..color = const Color(0xff4b89ff));
  }

  @override
  bool shouldRepaint(_LiveContent oldDelegate) => oldDelegate.tick != tick;
}

Future<void> _pumpSurface(WidgetTester tester, Widget child) async {
  final budget = GlassBackdropBudget();
  addTearDown(budget.dispose);
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.windows),
      home: GlassScope(
        data: GlassScopeData(appearance: GlassAppearance.fallback, budget: budget),
        child: Center(
          child: SizedBox(
            width: 800,
            height: 600,
            child: AmbientBackdrop(child: GlassSurface(child: child)),
          ),
        ),
      ),
    ),
  );
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
  await tester.pump();
}

Future<ByteData> _ambientPixels(WidgetTester tester, AmbientSpec spec, {double dpr = 1}) async {
  final tokens = GlassTokens.resolve(
    brightness: Brightness.dark,
    platform: TargetPlatform.windows,
  ).copyWith(ambient: spec);
  await tester.pumpWidget(
    MaterialApp(
      themeAnimationDuration: Duration.zero,
      theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.windows).copyWith(extensions: [tokens]),
      home: MediaQuery(
        data: MediaQueryData(devicePixelRatio: dpr),
        child: const Center(
          child: SizedBox(width: 512, height: 128, child: AmbientBackdrop(key: ValueKey('capture'))),
        ),
      ),
    ),
  );
  // Allow engine image decoding/capture callbacks to finish outside fake time.
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
  await tester.pump();
  final boundary = tester.renderObject<RenderRepaintBoundary>(
    find.descendant(of: find.byKey(const ValueKey('capture')), matching: find.byType(RepaintBoundary)).first,
  );
  final image = (await tester.runAsync(() => boundary.toImage(pixelRatio: dpr)))!;
  final pixels = (await tester.runAsync(() => image.toByteData(format: ui.ImageByteFormat.rawRgba)))!;
  image.dispose();
  return pixels;
}

void main() {
  testWidgets('live content repaint leaves static glass shadows, fill and ambient paint untouched', (tester) async {
    final tick = ValueNotifier(0);
    addTearDown(tick.dispose);
    final content = _LiveContent(tick);
    await _pumpSurface(tester, CustomPaint(painter: content));
    final counters = <String, _PaintCount>{};
    for (final name in ['ambient', 'fill', 'shadow']) {
      final render = tester.renderObject<RenderCustomPaint>(
        find.byWidgetPredicate(
          (widget) =>
              widget is CustomPaint &&
              switch (name) {
                'ambient' => widget.painter is AmbientPainter,
                'fill' => widget.painter is GlassFillPainter,
                _ => widget.painter is GlassShadowPainter,
              },
        ),
      );
      final counter = _PaintCount(render.painter!);
      counters[name] = counter;
      render.painter = counter;
    }
    await tester.pump();
    for (final counter in counters.values) {
      counter.count = 0;
    }
    content.count = 0;
    for (var i = 1; i <= 8; i++) {
      tick.value = i;
      await tester.pump();
    }
    debugPrint(
      'glass paints: content=${content.count}, ambient=${counters['ambient']!.count}, fill=${counters['fill']!.count}, shadow=${counters['shadow']!.count}',
    );
    expect(content.count, 8);
    expect(counters['ambient']!.count, 0);
    expect(counters['fill']!.count, 0);
    expect(counters['shadow']!.count, 0);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  test('ambient repaint detects changed gradient blobs and local profile cues', () {
    final original = AmbientSpec.resolve(Brightness.dark);
    final old = AmbientPainter(spec: original);
    expect(AmbientPainter(spec: AmbientSpec.resolve(Brightness.dark)).shouldRepaint(old), isFalse);
    final changedBlob = AmbientSpec(
      base: original.base,
      blobs: [
        AmbientBlob(
          center: original.blobs.first.center,
          radius: original.blobs.first.radius,
          color: const Color(0x66224466),
        ),
        ...original.blobs.skip(1),
      ],
      localCueBlob: original.localCueBlob,
      grain: original.grain,
    );
    expect(AmbientPainter(spec: changedBlob).shouldRepaint(old), isTrue);
    final changedCue = AmbientSpec(
      base: original.base,
      blobs: original.blobs,
      localCueBlob: const AmbientBlob(center: Offset(0.3, 0.5), radius: 0.2, color: Color(0x66333333)),
      grain: original.grain,
    );
    expect(
      AmbientPainter(
        spec: changedCue,
        localProfileCue: true,
      ).shouldRepaint(AmbientPainter(spec: original, localProfileCue: true)),
      isTrue,
    );
  });

  test('Windows frosted filter reuses unchanged blur and saturation objects across paints', () {
    final render = RenderGlassBackdrop(
      sigma: 20,
      saturation: 1.5,
      refraction: GlassRefraction.none,
      program: null,
      cornerRadius: 18,
      devicePixelRatio: 1,
    );
    addTearDown(render.dispose);
    const context = ImageFilterContext(bounds: Rect.fromLTWH(0, 0, 800, 600));
    final filters = HashSet<ui.ImageFilter>.identity();
    for (var i = 0; i < 8; i++) {
      filters.add(render.filterConfig.resolve(context));
    }
    debugPrint('unchanged glass filter allocations: ${filters.length}');
    expect(filters, hasLength(1));
    final initial = filters.single;
    render.sigma = 10;
    final resizedBlur = render.filterConfig.resolve(context);
    expect(identical(resizedBlur, initial), isFalse);
    render.saturation = 1;
    expect(identical(render.filterConfig.resolve(context), resizedBlur), isFalse);
  });

  testWidgets('ambient dither covers the dark gradient rather than sparse isolated points', (tester) async {
    const base = Color(0xff141820);
    const cue = AmbientBlob(center: Offset.zero, radius: 1, color: Color(0x00000000));
    const plain = AmbientSpec(base: base, blobs: [], localCueBlob: cue, grain: 0);
    const grain = AmbientSpec(base: base, blobs: [], localCueBlob: cue, grain: 0.015);
    final untextured = await _ambientPixels(tester, plain);
    final textured = await _ambientPixels(tester, grain);
    var changed = 0;
    var sumDelta = 0;
    for (var i = 0; i < textured.lengthInBytes; i += 4) {
      if (textured.getUint8(i) != untextured.getUint8(i)) changed++;
      sumDelta += textured.getUint8(i) - untextured.getUint8(i);
      expect(textured.getUint8(i + 3), 255);
    }
    final pixels = textured.lengthInBytes ~/ 4;
    debugPrint(
      'ambient dither density: ${(changed / pixels * 100).toStringAsFixed(2)}%, mean red delta=${sumDelta / pixels}',
    );
    expect(changed / pixels, greaterThan(0.35));
    expect((sumDelta / pixels).abs(), lessThan(1));
    final repeat = await _ambientPixels(tester, grain);
    expect(
      repeat.buffer.asUint8List(),
      textured.buffer.asUint8List(),
      reason: 'static grain never animates or changes on unrelated rebuilds',
    );
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('grain reuses one small texture and shades physical pixels at high display scale', (tester) async {
    const cue = AmbientBlob(center: Offset.zero, radius: 1, color: Color(0x00000000));
    const plain = AmbientSpec(base: Color(0xff141820), blobs: [], localCueBlob: cue, grain: 0);
    const grain = AmbientSpec(base: Color(0xff141820), blobs: [], localCueBlob: cue, grain: 0.015);
    AmbientPainter painter() =>
        tester.widget<CustomPaint>(find.byKey(const ValueKey('ambient-backdrop'))).painter! as AmbientPainter;
    await _ambientPixels(tester, grain);
    final tile = painter().grainTile!;
    expect(tile.width, 128);
    expect(tile.height, 128);
    final untextured = await _ambientPixels(tester, plain, dpr: 2);
    final textured = await _ambientPixels(tester, grain, dpr: 2);
    expect(identical(painter().grainTile, tile), isTrue);
    expect(painter().devicePixelRatio, 2);
    var changed = 0;
    for (var i = 0; i < textured.lengthInBytes; i += 4) {
      if (textured.getUint8(i) != untextured.getUint8(i)) changed++;
    }
    expect(changed / (textured.lengthInBytes ~/ 4), greaterThan(0.35));
    final render = tester.renderObject<RenderCustomPaint>(find.byKey(const ValueKey('ambient-backdrop')));
    final counter = _PaintCount(render.painter!);
    render.painter = counter;
    await tester.pump();
    counter.count = 0;
    await tester.pump(const Duration(seconds: 10));
    expect(counter.count, 0, reason: 'the texture adds no timer or idle animation');
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
