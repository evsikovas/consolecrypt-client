import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/terminal/terminal_output.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:xterm/xterm.dart';

List<Object> terminalState(Terminal terminal) => [
  terminal.mainBuffer.cursorX,
  terminal.mainBuffer.cursorY,
  terminal.altBuffer.cursorX,
  terminal.altBuffer.cursorY,
  terminal.isUsingAltBuffer,
  terminal.bracketedPasteMode,
  for (final buffer in [terminal.mainBuffer, terminal.altBuffer])
    [
      for (final line in buffer.lines.toList()) [line.isWrapped, line.length, line.data.toList()],
    ],
];

void main() {
  test('incremental UTF-8 and every UTF-16 slice preserve ANSI, Cyrillic and emoji exactly', () async {
    const text = 'начало 🌐\x1b[38;2;12;34;56mцвет 😀\x1b[0m\r\n\x1b]2;Заголовок🚀\x07конец';
    final bytes = utf8.encode(text);
    final parts = await yieldingTerminalOutput(
      Stream<List<int>>.fromIterable(bytes.map((byte) => [byte])),
      maxCodeUnitsPerTurn: 2,
    ).toList();
    expect(parts.join(), text);
    expect(parts, contains('\x1b[38;2;12;34;56m'));
    expect(parts, contains('\x1b]2;Заголовок🚀\x07'));
    expect(parts.any((part) => part == '🌐'), isTrue);
    expect(parts.any((part) => part.contains('\ufffd')), isFalse);
  });

  test('malformed and unfinished UTF-8 is flushed exactly once on source completion', () async {
    final bytes = [0x61, 0xff, 0xe2, 0x82, 0xac, 0x62, 0xf0, 0x9f];
    final parts = await yieldingTerminalOutput(
      Stream<List<int>>.fromIterable(bytes.map((byte) => [byte])),
      maxCodeUnitsPerTurn: 2,
    ).toList();
    expect(parts.join(), utf8.decode(bytes, allowMalformed: true));
  });

  test('real xterm cells, attributes, modes, titles and replies match an unsliced parser', () async {
    const text =
        '\x1b]2;Заголовок 🌐\x1b\\'
        'one\r\n\x1b[1;38;2;40;50;60;48;5;33mКириллица 😀\x1b[0m\r\n'
        '\x1b[?2004h\x1b[?1049h\x1b[2J\x1b[Hальт 🚀\x1b[6n\x1b[?1049l'
        '\x1b[2;3HXYZ\x1b[1D!\x1b[K\x1b[6n\x1b]2;Финал\x07';
    final expectedTitles = <String>[];
    final actualTitles = <String>[];
    final expectedReplies = <String>[];
    final actualReplies = <String>[];
    final expected = Terminal(maxLines: 200, onTitleChange: expectedTitles.add, onOutput: expectedReplies.add)
      ..resize(32, 8);
    final actual = Terminal(maxLines: 200, onTitleChange: actualTitles.add, onOutput: actualReplies.add)..resize(32, 8);
    expected.write(text);
    await for (final part in yieldingTerminalOutput(
      Stream<List<int>>.fromIterable([utf8.encode(text)]),
      maxCodeUnitsPerTurn: 3,
    )) {
      actual.write(part);
    }
    expect(terminalState(actual), terminalState(expected));
    expect(actualTitles, expectedTitles);
    expect(actualReplies, expectedReplies);
  });

  for (final fragmented in [false, true]) {
    test('a long OSC is parsed once, including when split across native frames ($fragmented)', () async {
      final title = List.filled(1024 * 1024, 'x').join();
      final token = '\x1b]2;$title\x1b\\';
      final bytes = utf8.encode(token);
      final frames = fragmented
          ? [
              for (var start = 0; start < bytes.length; start += 65536)
                bytes.sublist(start, (start + 65536).clamp(0, bytes.length)),
            ]
          : [bytes];
      final expectedTitles = <String>[];
      final actualTitles = <String>[];
      final expected = Terminal(onTitleChange: expectedTitles.add)..write(token);
      final actual = Terminal(onTitleChange: actualTitles.add);
      var calls = 0;
      var joinedLength = 0;
      await for (final part in yieldingTerminalOutput(Stream<List<int>>.fromIterable(frames))) {
        calls++;
        joinedLength += part.length;
        expect(part, token);
        actual.write(part);
      }
      expect(calls, 1, reason: 'partial OSC must not make xterm rescan a growing prefix hundreds of times');
      expect(joinedLength, token.length);
      expect(actualTitles, expectedTitles);
      expect(actualTitles, [title]);
      expect(terminalState(actual), terminalState(expected));
    });
  }

  test('a completed OSC exactly at the retention limit is preserved atomically', () async {
    final title = List.filled(terminalEscapeTokenLimit - 5, 'x').join();
    final token = '\x1b]2;$title\x07';
    expect(token.length, terminalEscapeTokenLimit);
    final parts = await yieldingTerminalOutput(Stream.value(utf8.encode(token))).toList();
    expect(parts, [token]);
  });

  test('valid indexed and RGB color channel boundaries keep upstream packed values', () {
    final terminal = Terminal()..write('\x1b[38;5;0;48;5;255mX');
    expect(terminal.cursor.foreground, CellColor.palette);
    expect(terminal.cursor.background, CellColor.palette | 255);
    terminal.write('\x1b[38;2;0;255;0;48;2;255;0;255mY');
    expect(terminal.cursor.foreground, CellColor.rgb | 0x00ff00);
    expect(terminal.cursor.background, CellColor.rgb | 0xff00ff);
    expect(terminal.mainBuffer.lines[0].getForeground(0), CellColor.palette);
    expect(terminal.mainBuffer.lines[0].getBackground(0), CellColor.palette | 255);
    expect(terminal.mainBuffer.lines[0].getForeground(1), CellColor.rgb | 0x00ff00);
    expect(terminal.mainBuffer.lines[0].getBackground(1), CellColor.rgb | 0xff00ff);
  });

  for (final directive in [
    '38',
    '38;2',
    '38;2;12',
    '38;2;12;34',
    '38;5',
    '48',
    '48;2',
    '48;2;12',
    '48;2;12;34',
    '48;5',
  ]) {
    test('malformed SGR $directive preserves prior style and subsequent text', () async {
      const following = 'still usable Кириллица 🌐\x1b[0m\x1b[38;2;12;34;56;48;5;33mvalid\x1b[0m';
      final expected = Terminal()..write('\x1b[1;3m$following');
      final actual = Terminal();
      await for (final part in yieldingTerminalOutput(Stream.value(utf8.encode('\x1b[1;3;${directive}m$following')))) {
        actual.write(part);
      }
      expect(terminalState(actual), terminalState(expected));
      actual.write('\r\nnext command');
      expected.write('\r\nnext command');
      expect(terminalState(actual), terminalState(expected));
    });
  }

  for (final directive in ['38;5;256', '48;5;999', '38;2;256;0;0', '48;2;0;256;0', '38;2;0;0;999']) {
    test('out-of-range SGR $directive preserves prior valid colors and text', () async {
      const following = 'usable Кириллица 🌐\x1b[38;5;255;48;2;0;255;0mvalid\x1b[0m';
      final expected = Terminal()..write('\x1b[31;44m$following');
      final actual = Terminal();
      await for (final part in yieldingTerminalOutput(
        Stream.value(utf8.encode('\x1b[31;44;${directive}m$following')),
      )) {
        actual.write(part);
      }
      expect(terminalState(actual), terminalState(expected));
    });
  }

  for (final directive in ['38;5;256', '48;5;999']) {
    testWidgets('out-of-range palette $directive cannot break TerminalView paint', (tester) async {
      final terminal = Terminal()..write('\x1b[31;44m\x1b[${directive}mnormal text');
      await tester.pumpWidget(MaterialApp(home: TerminalView(terminal)));
      expect(tester.takeException(), isNull);
      terminal.write('\r\nstill usable');
      await tester.pump();
      expect(tester.takeException(), isNull);
    });
  }

  test('malformed OSC, charset and unknown ESC keep xterm scalar semantics', () async {
    const text = 'a\x1b]2;discard\x1b🌐b\x1b(😀c\x1b🚀d\x1b[?25l\x1b]2;final\x07';
    final expectedTitles = <String>[];
    final actualTitles = <String>[];
    final expected = Terminal(onTitleChange: expectedTitles.add)..write(text);
    final actual = Terminal(onTitleChange: actualTitles.add);
    final parts = await yieldingTerminalOutput(
      Stream<List<int>>.fromIterable(utf8.encode(text).map((byte) => [byte])),
      maxCodeUnitsPerTurn: 2,
    ).toList();
    expect(parts.join(), text);
    for (final part in parts) {
      actual.write(part);
    }
    expect(terminalState(actual), terminalState(expected));
    expect(actualTitles, expectedTitles);
  });

  for (final osc in [false, true]) {
    for (final terminated in [false, true]) {
      test('over-limit ${osc ? 'OSC' : 'CSI'} drops only that token (${terminated ? 'terminated' : 'EOF'})', () async {
        final token =
            '${osc ? '\x1b]2;' : '\x1b['}${List.filled(terminalEscapeTokenLimit + 1, osc ? 'x' : ';').join()}';
        final text = 'before$token${terminated ? '${osc ? '\x07' : 'm'}after🌐\x1b[32mgreen\x1b[0m' : ''}';
        final bytes = utf8.encode(text);
        final titles = <String>[];
        final actual = Terminal(onTitleChange: titles.add);
        final expected = Terminal()..write('before${terminated ? 'after🌐\x1b[32mgreen\x1b[0m' : ''}');
        final parts = await yieldingTerminalOutput(
          Stream<List<int>>.fromIterable([
            for (var start = 0; start < bytes.length; start += 65536)
              bytes.sublist(start, (start + 65536).clamp(0, bytes.length)),
          ]),
        ).toList();
        expect(parts.join(), 'before${terminated ? 'after🌐\x1b[32mgreen\x1b[0m' : ''}');
        expect(parts.every((part) => part.length <= 4096), isTrue);
        for (final part in parts) {
          actual.write(part);
        }
        expect(titles, isEmpty);
        expect(terminalState(actual), terminalState(expected));
      });
    }
  }

  for (final tail in ['\x1b', '\x1b[38;2;12;', '\x1b]2;unfinished🌐', '\x1b]2;unfinished\x1b', '\x1b(']) {
    test('incomplete EOF escape is retained and flushed once (${tail.length} units)', () async {
      final text = 'before$tail';
      final expected = Terminal()..write(text);
      final actual = Terminal();
      final parts = await yieldingTerminalOutput(
        Stream<List<int>>.fromIterable(utf8.encode(text).map((byte) => [byte])),
        maxCodeUnitsPerTurn: 2,
      ).toList();
      expect(parts.join(), text);
      expect(parts.last, tail);
      expect(parts.where((part) => part.contains('\x1b')), [tail]);
      for (final part in parts) {
        actual.write(part);
      }
      expect(terminalState(actual), terminalState(expected));
      // The next parser input behaves identically even after incomplete EOF.
      expected.write('34;56mnext\x07');
      actual.write('34;56mnext\x07');
      expect(terminalState(actual), terminalState(expected));
    });
  }

  test('pausing between source EOF and the final parser turn resumes completion', () async {
    final source = StreamController<List<int>>();
    final done = Completer<void>();
    final parts = <String>[];
    final subscription = yieldingTerminalOutput(source.stream).listen(parts.add, onDone: done.complete);
    addTearDown(subscription.cancel);
    await source.close();
    subscription.pause();
    await Future<void>.delayed(const Duration(milliseconds: 2));
    expect(done.isCompleted, isFalse);
    subscription.resume();
    await done.future.timeout(const Duration(seconds: 1));
    expect(parts, isEmpty);
  });

  test('cancelling a retained incomplete OSC releases it without a parser flush', () async {
    final scannedFrame = Completer<void>();
    var cancelled = false;
    final source = StreamController<List<int>>(
      onResume: () {
        if (!scannedFrame.isCompleted) scannedFrame.complete();
      },
      onCancel: () => cancelled = true,
    );
    final parts = <String>[];
    final subscription = yieldingTerminalOutput(source.stream).listen(parts.add);
    source.add(utf8.encode('\x1b]2;${List.filled(65536, 'x').join()}'));
    await scannedFrame.future.timeout(const Duration(seconds: 1));
    await subscription.cancel().timeout(const Duration(seconds: 1));
    await source.close();
    await Future<void>.delayed(const Duration(milliseconds: 2));
    expect(cancelled, isTrue);
    expect(parts, isEmpty);
  });

  test('a 2MiB attach snapshot yields a timer before parser completion', () async {
    final source = StreamController<List<int>>();
    final done = Completer<void>();
    final heartbeat = Completer<bool>();
    var complete = false;
    var parts = 0;
    var units = 0;
    final subscription = yieldingTerminalOutput(source.stream).listen(
      (part) {
        parts++;
        units += part.length;
        expect(part.length, lessThanOrEqualTo(4096));
        if (parts == 1) Timer.run(() => heartbeat.complete(!complete));
      },
      onDone: () {
        complete = true;
        done.complete();
      },
    );
    addTearDown(subscription.cancel);
    source.add(List.filled(2 * 1024 * 1024, 0x61));
    unawaited(source.close());
    expect(await heartbeat.future, isTrue, reason: 'window input/frame timers must get an event turn during output');
    await done.future;
    expect(parts, greaterThan(100));
    expect(units, 2 * 1024 * 1024);
  });

  test('only one decoded source event is consumed while downstream is paused', () async {
    var inputEvents = 0;
    var sourcePaused = false;
    final source = StreamController<List<int>>(
      onPause: () => sourcePaused = true,
      onResume: () => sourcePaused = false,
    );
    final first = Completer<void>();
    final done = Completer<void>();
    var parts = 0;
    var length = 0;
    late StreamSubscription<String> subscription;
    subscription =
        yieldingTerminalOutput(
          source.stream.map((bytes) {
            inputEvents++;
            return bytes;
          }),
        ).listen((part) {
          parts++;
          length += part.length;
          if (parts == 1) {
            subscription.pause();
            first.complete();
          }
        }, onDone: done.complete);
    addTearDown(subscription.cancel);
    source.add(List.filled(65536, 0x61));
    source.add(List.filled(65536, 0x62));
    unawaited(source.close());
    await first.future;
    await Future<void>.delayed(const Duration(milliseconds: 2));
    expect(inputEvents, 1);
    expect(sourcePaused, isTrue);
    expect(parts, 1);
    subscription.resume();
    await done.future;
    expect(inputEvents, 2);
    expect(length, 2 * 65536);
  });

  test('cancellation after one slice releases pending output and cancels upstream', () async {
    var cancelled = false;
    final source = StreamController<List<int>>(onCancel: () => cancelled = true);
    final stopped = Completer<void>();
    var parts = 0;
    late StreamSubscription<String> subscription;
    subscription = yieldingTerminalOutput(source.stream).listen((part) {
      parts++;
      unawaited(subscription.cancel().then((_) => stopped.complete()));
    });
    source.add(List.filled(2 * 1024 * 1024, 0x61));
    await stopped.future;
    await Future<void>.delayed(const Duration(milliseconds: 2));
    expect(cancelled, isTrue);
    expect(parts, 1);
    await source.close();
  });

  test('an idle source cancels without waiting for another native output frame', () async {
    var cancelled = false;
    final source = StreamController<List<int>>(onCancel: () => cancelled = true);
    final subscription = yieldingTerminalOutput(source.stream).listen((_) {});
    await subscription.cancel().timeout(const Duration(seconds: 1));
    expect(cancelled, isTrue);
    await source.close();
  });

  test('source errors are forwarded without dropping later output or duplicating decoder flush', () async {
    final source = StreamController<List<int>>();
    final parts = <String>[];
    final errors = <Object>[];
    final done = Completer<void>();
    yieldingTerminalOutput(source.stream).listen(parts.add, onError: errors.add, onDone: done.complete);
    source.add(utf8.encode('до🌐'));
    final error = StateError('synthetic');
    source.addError(error);
    source.add(utf8.encode('после'));
    source.add([0xe2]);
    unawaited(source.close());
    await done.future;
    expect(parts.join(), 'до🌐после\ufffd');
    expect(errors, [same(error)]);
  });

  test('parser slice limit must fit at least one supplementary Unicode scalar', () {
    expect(() => yieldingTerminalOutput(const Stream.empty(), maxCodeUnitsPerTurn: 1), throwsRangeError);
  });

  for (final switchProfile in [false, true]) {
    test('pending parser turns stop after ${switchProfile ? 'profile switch' : 'tab close'}', () async {
      final backend = MockBackend(config: const MockConfig.test());
      final service = _FloodTerminal();
      final container = ProviderContainer(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          terminalServiceProvider.overrideWithValue(service),
        ],
      );
      addTearDown(container.dispose);
      addTearDown(backend.dispose);
      await backend.debugSignInDemoAndUnlock();
      final observed = container.listen(activeProfileProvider, (_, _) {});
      addTearDown(observed.close);
      await pumpEventQueue();
      final controller = container.read(terminalTabsProvider.notifier);
      final host = (await backend.inventory.watchHosts().first).first;
      final tab = await controller.open(host);
      final first = Completer<void>();
      tab.terminal.addListener(() {
        if (!first.isCompleted) first.complete();
      });
      service.output.add(Uint8List.fromList(List.filled(2 * 1024 * 1024, 0x61)));
      await first.future;
      if (switchProfile) {
        await backend.debugCreateUnlockedLocalProfile();
        await pumpEventQueue();
      } else {
        await controller.close(tab).timeout(const Duration(seconds: 1));
      }
      expect(container.read(terminalTabsProvider).tabs, isEmpty);
      expect(service.cancelled, isTrue);
      expect(service.closeCalls, 1);
      final state = terminalState(tab.terminal);
      await Future<void>.delayed(const Duration(milliseconds: 2));
      expect(
        terminalState(tab.terminal),
        state,
        reason: 'cancelled work may not parse into a closed/profile-switched tab',
      );
      expect(
        tab.terminal.mainBuffer.lines.length,
        lessThan(2000),
        reason: 'the full 2MiB must not finish before cancellation',
      );
    });
  }
}

final class _FloodTerminal implements TerminalService {
  _FloodTerminal() {
    output = StreamController<Uint8List>(onCancel: () => cancelled = true);
  }
  late final StreamController<Uint8List> output;
  final events = StreamController<TerminalEvent>();
  bool cancelled = false;
  int closeCalls = 0;
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) async =>
      TerminalSessionHandle(id: TerminalSessionId.generate(), output: output.stream, events: events.stream);
  @override
  Future<void> close(TerminalSessionId id) async {
    closeCalls++;
    await output.close();
    await events.close();
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async {}
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
  @override
  Future<void> reconnect(TerminalSessionId id) async {}
  @override
  Future<void> answerHostKey(TerminalSessionId id, HostKeyDecision decision) async {}
  @override
  Future<void> answerPassword(TerminalSessionId id, SecretText? password) async {
    password?.wipe();
  }
}
