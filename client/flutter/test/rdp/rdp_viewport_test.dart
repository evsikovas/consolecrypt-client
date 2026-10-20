import 'package:consolecrypt/rdp/rdp_viewport.dart';
import 'package:flutter/painting.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('render and hit-test share one transform at every zoom and aspect ratio', () {
    for (final screen in [const Size(390, 844), const Size(844, 390), const Size(1200, 800)]) {
      final view = RdpViewport()..configure(screen, 1920, 1080);
      for (final zoom in [1.0, 1.5, 2.0, 4.0]) {
        view.reset();
        view.magnify(zoom, screen.center(Offset.zero));
        for (final point in [Offset.zero, const Offset(1919, 1079), const Offset(960, 540)]) {
          final result = view.toRemote(view.toLocal(point));
          expect(result, isNotNull);
          expect((result! - point).distance, lessThan(.0001));
        }
      }
    }
  });

  test('zoom anchors the same remote pixel and clamps to 1–4', () {
    final view = RdpViewport()..configure(const Size(800, 600), 1600, 1200);
    const focal = Offset(250, 200);
    final before = view.toRemote(focal);
    view.magnify(2, focal);
    expect(view.toRemote(focal), before);
    view.magnify(100, focal);
    expect(view.zoom, 4);
    view.magnify(.001, focal);
    expect(view.zoom, 1);
    expect(view.offset, Offset.zero);
  });

  test('cursor follows near edges without exposing blank space', () {
    final view = RdpViewport()..configure(const Size(800, 600), 1600, 1200);
    view.magnify(4, const Offset(400, 300));
    view.follow(const Offset(1590, 1190));
    expect(view.visible.contains(view.toLocal(const Offset(1590, 1190))), isTrue);
    expect(view.rect.right, greaterThanOrEqualTo(view.size.width));
    expect(view.rect.bottom, greaterThanOrEqualTo(view.size.height));
    view.follow(Offset.zero);
    expect(view.toLocal(Offset.zero), Offset.zero);
  });

  test('keyboard overlays a stable surface and restores original zoom', () {
    final view = RdpViewport()..configure(const Size(390, 844), 600, 1280);
    final before = view.rect;
    view.keyboard(340, const Offset(300, 1100));
    expect(view.size, const Size(390, 844));
    expect(view.zoom, 1.5);
    expect(view.toLocal(const Offset(300, 1100)).dy, lessThan(504));
    view.keyboard(0, const Offset(300, 1100));
    expect(view.zoom, 1);
    expect(view.rect, before);
  });

  test('phone resolution preserves orientation and native limits', () {
    for (final size in [const Size(390, 844), const Size(844, 390), const Size(1000, 3000)]) {
      final (width, height) = rdpPhoneResolution(size);
      expect(width.isEven && height.isEven, isTrue);
      expect(width, inInclusiveRange(200, 4096));
      expect(height, inInclusiveRange(200, 2160));
      expect((width / height - size.aspectRatio).abs(), lessThan(.005));
    }
  });
}
