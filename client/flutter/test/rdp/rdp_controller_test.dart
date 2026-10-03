import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/rdp/rdp_controller.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter_test/flutter_test.dart';

import 'fake_rdp_service.dart';

final _options = RdpConnectionOptions(address: 'example.test', username: 'demo');
Future<RdpTab> _connect(RdpWorkspaceController controller) async =>
    (await controller.connect(_options, Uint8List(0), List.filled(64, 'a').join(), isCurrent: () => true))!;

void main() {
  testWidgets('failed priority release closes input and a late poll cannot revive it', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    final poll = Completer<RdpPollResult>();
    service.pollGate = poll;
    await tester.pump(const Duration(milliseconds: 67));
    service.inputFailure = const RdpFailure('input_queue_full');
    controller.cancelInteraction(tab);
    await tester.pump();
    expect(tab.status.phase, RdpPhase.failed);
    expect(service.disconnected, [tab.info.id]);
    poll.complete(const RdpPollResult(status: RdpStatus(RdpPhase.connected)));
    await tester.pump();
    controller.send(tab, const [RdpUnicodeInput('blocked')]);
    await tester.pump();
    expect(tab.status.phase, RdpPhase.failed);
    expect(service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>(), isEmpty);
    controller.dispose();
    await tester.pump();
  });

  testWidgets(
    'visibility cancellation bypasses clipboard FIFO and fences fresh input until native cancellation settles',
    (tester) async {
      final service = FakeRdpService();
      final controller = RdpWorkspaceController(service);
      final tab = await _connect(controller);
      await tester.pump(const Duration(milliseconds: 67));
      final heldPaste = Completer<void>();
      final cancel = Completer<void>();
      service.inputGate = cancel;
      final paste = controller.queueClipboardPaste(tab, () => heldPaste.future, isCurrent: () => true);
      try {
        await tester.pump();
        controller.send(tab, const [RdpUnicodeInput('stale')]);
        controller.setVisible(false);
        expect(service.inputs.single.single, isA<RdpReleaseAllInput>());
        controller.setVisible(true);
        controller.send(tab, const [RdpUnicodeInput('fresh')]);
        heldPaste.complete();
        await paste;
        await tester.pump();
        expect(service.inputs, hasLength(1));
        cancel.complete();
        await tester.pump();
        expect(service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().single.text, 'fresh');
        expect(tab.status.phase, RdpPhase.connected);
      } finally {
        if (!heldPaste.isCompleted) heldPaste.complete();
        if (!cancel.isCompleted) cancel.complete();
        await paste;
        controller.dispose();
        await tester.pump();
      }
    },
  );

  testWidgets('clipboard transaction holds one FIFO place and cancellation does not poison later input', (
    tester,
  ) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    service.inputGate = Completer<void>();
    controller.send(tab, const [RdpScancodeInput(0x1d, down: false)]);
    var started = false;
    var current = true;
    final pending = controller.queueClipboardPaste(tab, () async {
      started = true;
    }, isCurrent: () => current);
    final cancellation = expectLater(pending, throwsA(isA<RdpFailure>()));
    controller.send(tab, const [RdpUnicodeInput('x')]);
    await tester.pump();
    expect(started, isFalse);
    expect(service.inputs, hasLength(1));
    current = false;
    service.inputGate!.complete();
    await cancellation;
    await tester.pump();
    expect(started, isFalse);
    expect(service.inputs, hasLength(2));
    expect(service.inputs.last.single, isA<RdpUnicodeInput>());
    expect(tab.status.phase, RdpPhase.connected);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('clipboard ACK failure leaves session and input FIFO usable', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    final pending = controller.queueClipboardPaste(tab, () async {
      throw const RdpFailure('clipboard_unavailable');
    }, isCurrent: () => true);
    await expectLater(pending, throwsA(isA<RdpFailure>()));
    controller.send(tab, const [RdpUnicodeInput('Привет')]);
    await tester.pump();
    expect(service.inputs, hasLength(1));
    expect(tab.status.phase, RdpPhase.connected);
    expect(service.disconnected, isEmpty);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('clipboard-only permission change preserves an accepted folder', (tester) async {
    final service = FakeRdpService()
      ..nextPoll = const RdpPollResult(status: RdpStatus(RdpPhase.connected), folderState: RdpFolderState.ready);
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await controller.setPermissions(
      tab,
      const RdpSessionPermissions(directoryGrantId: 'folder'),
      directory: const RdpDirectoryGrant(id: 'folder', name: 'Demo documents'),
    );
    await tester.pump(const Duration(milliseconds: 67));
    expect(tab.folderState, RdpFolderState.ready);
    await controller.setPermissions(tab, tab.permissions.copyWith(clipboardEnabled: true));
    expect(tab.folderState, RdpFolderState.ready);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('old poll cannot mark a replaced folder ready; denial leaves the desktop connected', (tester) async {
    final service = FakeRdpService()..pollGate = Completer();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    await controller.setPermissions(
      tab,
      const RdpSessionPermissions(directoryGrantId: 'new-folder'),
      directory: const RdpDirectoryGrant(id: 'new-folder', name: 'Demo new folder'),
    );
    service.pollGate!.complete(
      const RdpPollResult(status: RdpStatus(RdpPhase.connected), folderState: RdpFolderState.ready),
    );
    await tester.pump();
    expect(tab.folderState, RdpFolderState.pending);
    service.pollGate = null;
    service.nextPoll = const RdpPollResult(status: RdpStatus(RdpPhase.connected), folderState: RdpFolderState.denied);
    await tester.pump(const Duration(milliseconds: 67));
    expect(tab.folderState, RdpFolderState.denied);
    expect(tab.status.phase, RdpPhase.connected);
    controller.send(tab, const [RdpUnicodeInput('Привет')]);
    await tester.pump();
    expect(service.inputs, hasLength(1));
    expect(service.disconnected, isEmpty);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('permission state changes only after native acknowledgement, failures preserve prior flags', (
    tester,
  ) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    service.permissionGate = Completer();
    final update = controller.setPermissions(tab, const RdpSessionPermissions(clipboardEnabled: true));
    await tester.pump();
    expect(tab.permissions.clipboardEnabled, isFalse);
    service.permissionGate!.complete();
    await update;
    expect(tab.permissions.clipboardEnabled, isTrue);
    service.permissionFailure = const RdpFailure('permissions');
    await expectLater(controller.setPermissions(tab, const RdpSessionPermissions()), throwsA(isA<RdpFailure>()));
    expect(tab.permissions.clipboardEnabled, isTrue);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('closing during permission update cannot adopt a late grant', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    service.permissionGate = Completer();
    final update = controller.setPermissions(
      tab,
      const RdpSessionPermissions(directoryGrantId: 'pending-grant'),
      directory: const RdpDirectoryGrant(id: 'pending-grant', name: 'Demo folder'),
    );
    final assertion = expectLater(update, throwsA(isA<RdpFailure>()));
    await tester.pump();
    await controller.close(tab);
    service.permissionGate!.complete();
    await assertion;
    expect(tab.directory, isNull);
    expect(tab.permissions.directoryGrantId, isNull);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('late cancelled connect is disconnected and never becomes a tab', (tester) async {
    final service = FakeRdpService()..connectGate = Completer();
    final controller = RdpWorkspaceController(service);
    final connecting = controller.connect(_options, Uint8List(0), List.filled(64, 'a').join(), isCurrent: () => true);
    await controller.closeAll();
    service.connectGate!.complete(RdpSessionInfo(id: 'late', width: 1280, height: 720));
    expect(await connecting, isNull);
    expect(controller.tabs, isEmpty);
    expect(service.disconnected, ['late']);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('four-session limit includes pending connects', (tester) async {
    final service = FakeRdpService();
    final workspace = RdpWorkspaceController(service);
    for (var i = 0; i < 3; i++) {
      await _connect(workspace);
    }
    service.connectGate = Completer();
    final fourth = _connect(workspace);
    expect(workspace.canConnect, isFalse);
    await expectLater(_connect(workspace), throwsA(isA<RdpFailure>()));
    expect(service.options, hasLength(4));
    service.connectGate!.complete(RdpSessionInfo(id: 'fourth', width: 1280, height: 720));
    await fourth;
    workspace.dispose();
    await tester.pump();
  });

  testWidgets('native input is FIFO and queued commands drop on close', (tester) async {
    final service = FakeRdpService()..inputGate = Completer();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    controller.send(tab, const [RdpUnicodeInput('п')]);
    controller.send(tab, const [RdpUnicodeInput('ривет')]);
    await tester.pump();
    expect(service.inputs, hasLength(1));
    await controller.close(tab);
    service.inputGate!.complete();
    await tester.pump();
    expect(service.inputs, hasLength(1));
    expect(service.disconnected, [tab.info.id]);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('only one poll in flight and late lock frame is discarded', (tester) async {
    final service = FakeRdpService()..pollGate = Completer();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    await tester.pump(const Duration(seconds: 1));
    expect(service.pollCount, 1);
    await controller.closeAll();
    service.pollGate!.complete(
      RdpPollResult(
        status: const RdpStatus(RdpPhase.connected),
        frame: RdpFrame(sequence: 0, width: 1, height: 1, rgba: Uint8List(4)),
      ),
    );
    await tester.pump();
    expect(tab.closed, isTrue);
    expect(tab.frame, isNull);
    expect(controller.tabs, isEmpty);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('connecting state keeps polling until authentication completes; input stays gated', (tester) async {
    final service = FakeRdpService()..nextPoll = const RdpPollResult(status: RdpStatus(RdpPhase.connecting));
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    controller.send(tab, const [RdpUnicodeInput('blocked')]);
    await tester.pump(const Duration(milliseconds: 67));
    await tester.pump(const Duration(milliseconds: 67));
    expect(service.pollCount, 2);
    expect(service.inputs, isEmpty);
    expect(tab.status.phase, RdpPhase.connecting);
    service.nextPoll = RdpPollResult(
      status: const RdpStatus(RdpPhase.connected),
      frame: RdpFrame(sequence: 0, width: 1, height: 1, rgba: Uint8List(4)),
    );
    await tester.pump(const Duration(milliseconds: 67));
    expect(tab.status.phase, RdpPhase.connected);
    expect(tab.frame, isNotNull);
    controller.send(tab, const [RdpUnicodeInput('accepted')]);
    await tester.pump();
    expect(service.inputs, hasLength(1));
    controller.dispose();
    await tester.pump();
  });

  testWidgets('non-increasing frames cannot replace a newer remote desktop', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    service.nextPoll = RdpPollResult(
      status: const RdpStatus(RdpPhase.connected),
      frame: RdpFrame(sequence: 2, width: 1, height: 1, rgba: Uint8List(4)),
    );
    await tester.pump(const Duration(milliseconds: 67));
    service.nextPoll = RdpPollResult(
      status: const RdpStatus(RdpPhase.connected),
      frame: RdpFrame(sequence: 1, width: 1, height: 1, rgba: Uint8List(4)),
    );
    await tester.pump(const Duration(milliseconds: 67));
    expect(tab.frame!.sequence, 2);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('unsupported resize is a nonfatal notice; subsequent Unicode still reaches session', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    await tester.pump(const Duration(milliseconds: 67));
    service.inputFailure = const RdpFailure('resize_unavailable');
    controller.send(tab, const [RdpResizeInput(1920, 1080)]);
    await tester.pump();
    expect(tab.status.phase, RdpPhase.connected);
    expect(tab.noticeCode, 'resize_unavailable');
    expect(service.disconnected, isEmpty);
    service.inputFailure = null;
    controller.send(tab, const [RdpUnicodeInput('привет')]);
    await tester.pump();
    expect(service.inputs, hasLength(2));
    expect(tab.status.phase, RdpPhase.connected);
    controller.dispose();
    await tester.pump();
  });

  testWidgets('hidden shell branch pauses polling and unchanged idle status does not repaint', (tester) async {
    final service = FakeRdpService();
    final controller = RdpWorkspaceController(service);
    final tab = await _connect(controller);
    var notifications = 0;
    tab.addListener(() => notifications++);
    await tester.pump(const Duration(milliseconds: 67));
    expect(notifications, 1);
    await tester.pump(const Duration(milliseconds: 67));
    expect(notifications, 1);
    controller.setVisible(false);
    final polls = service.pollCount;
    await tester.pump(const Duration(seconds: 1));
    expect(service.pollCount, polls);
    controller.setVisible(true);
    await tester.pump(const Duration(milliseconds: 67));
    expect(service.pollCount, polls + 1);
    controller.dispose();
    await tester.pump();
  });
}
