import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/terminal_attachment_relay.dart';
import 'package:consolecrypt/core/models/terminal.dart';
import 'package:consolecrypt/src/rust/api/ssh.dart' as native;
import 'package:consolecrypt/terminal/terminal_output.dart';
import 'package:flutter_test/flutter_test.dart';

TerminalOutputFrame output(List<int> bytes, {bool reset = false}) =>
    TerminalOutputFrame(Uint8List.fromList(bytes), reset: reset);
native.TerminalFrame frame(native.TerminalFrameKind kind, [String text = '']) =>
    native.TerminalFrame(kind: kind, data: Uint8List.fromList(utf8.encode(text)));

void main() {
  test('reset stays ordered after queued output and discards partial ANSI and UTF-8', () async {
    final before = 'old ' * 10000;
    final result = await yieldingTerminalFrames(
      Stream.fromIterable([
        output(utf8.encode(before)),
        output([...utf8.encode('\x1b]2;incomplete title'), 0xf0, 0x9f]),
        output(utf8.encode('новый 🌐\r\n'), reset: true),
        output(utf8.encode('after')),
      ]),
      maxCodeUnitsPerTurn: 7,
    ).toList();
    final split = result.indexWhere((f) => f.reset);
    expect(split, greaterThan(1));
    expect(result.take(split).map((f) => f.text).join(), before);
    expect(result.skip(split + 1).map((f) => f.text).join(), 'новый 🌐\r\nafter');
    expect(result.where((f) => f.reset), hasLength(1));
  });

  test('reattach starts while old cancellation is pending; snapshot reset precedes new live bytes', () async {
    final oldCancelled = Completer<void>();
    final old = StreamController<native.TerminalFrame>(onCancel: () => oldCancelled.future);
    final next = StreamController<native.TerminalFrame>();
    var attachments = 0;
    final received = <TerminalOutputFrame>[];
    final statuses = <native.TerminalFrame>[];
    var done = 0;
    final relay = TerminalAttachmentRelay(
      attach: () => attachments++ == 0 ? old.stream : next.stream,
      onOutput: received.add,
      onStatus: statuses.add,
      onError: (e) => fail('$e'),
      onDone: () => done++,
    )..start();
    old.add(frame(native.TerminalFrameKind.data, 'old'));
    old.add(frame(native.TerminalFrameKind.lagged));
    await pumpEventQueue();
    expect(attachments, 2, reason: 'must not await an idle old attachment');
    next.add(frame(native.TerminalFrameKind.data, 'snapshot'));
    next.add(frame(native.TerminalFrameKind.data, 'live'));
    old.add(frame(native.TerminalFrameKind.failed));
    await pumpEventQueue();
    expect(received.map((f) => utf8.decode(f.data)), ['old', 'snapshot', 'live']);
    expect(received.map((f) => f.reset), [false, true, false]);
    expect(statuses, isEmpty);
    expect(done, 0);
    await relay.cancel();
    next.add(frame(native.TerminalFrameKind.lagged));
    await pumpEventQueue();
    expect(attachments, 2, reason: 'closing the tab cannot reopen an attachment');
    oldCancelled.complete();
    await old.close();
    await next.close();
  });

  test('an empty replacement snapshot still resets the view exactly once', () async {
    final streams = List.generate(2, (_) => StreamController<native.TerminalFrame>());
    var index = 0;
    final received = <TerminalOutputFrame>[];
    final relay = TerminalAttachmentRelay(
      attach: () => streams[index++].stream,
      onOutput: received.add,
      onStatus: (_) {},
      onError: (e) => fail('$e'),
      onDone: () {},
    )..start();
    streams.first.add(frame(native.TerminalFrameKind.lagged));
    await pumpEventQueue();
    streams.last.add(frame(native.TerminalFrameKind.data));
    streams.last.add(frame(native.TerminalFrameKind.data, 'ready'));
    await pumpEventQueue();
    expect(received.map((f) => f.reset), [true, false]);
    await relay.cancel();
    for (final stream in streams) {
      await stream.close();
    }
  });
}
