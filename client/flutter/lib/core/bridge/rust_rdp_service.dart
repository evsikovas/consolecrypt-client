import 'dart:typed_data';

import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/api/rdp.dart' as native;
import 'package:consolecrypt/src/rust/api/rdp_hosts.dart' as saved;

/// Raw RGBA frames cross FRB without JSON, base64, persistence or logging.
final class RustRdpService implements RdpService {
  const RustRdpService();

  Future<T> _guard<T>(Future<T> Function() operation) async {
    try {
      return await operation();
    } on BridgeError catch (error) {
      throw RdpFailure(error.code);
    }
  }

  @override
  Future<RdpCertificate> probeCertificate(String address, int port) => _guard(() async {
    final certificate = await native.rdpProbeCertificate(address: address, port: port);
    return RdpCertificate(address: certificate.address, port: certificate.port, sha256: certificate.sha256);
  });

  @override
  Future<RdpSessionInfo> connect(
    RdpConnectionOptions options, {
    required Uint8List passwordBytes,
    required String certificateSha256,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) => _guard(() async {
    final bytes = Uint8List.fromList(passwordBytes);
    try {
      final session = await native.rdpConnectWithPermissions(
        options: native.RdpConnection(
          address: options.address,
          port: options.port,
          username: options.username,
          domain: options.domain,
          width: options.width,
          height: options.height,
        ),
        password: bytes,
        certificateSha256: certificateSha256,
        permissions: _permissions(permissions),
      );
      return RdpSessionInfo(id: session.id, width: session.width, height: session.height);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  });

  native.RdpPermissions _permissions(RdpSessionPermissions value) => native.RdpPermissions(
    clipboardEnabled: value.clipboardEnabled,
    directoryGrantId: value.directoryGrantId,
    directoryWritable: value.directoryWritable,
  );

  @override
  Future<RdpSavedHostTicket> probeSavedHost(String hostId) => _guard(() async {
    final ticket = await saved.rdpSavedHostProbe(hostId: hostId);
    return RdpSavedHostTicket(
      hostId: ticket.hostId,
      name: ticket.name,
      options: RdpConnectionOptions(
        address: ticket.address,
        port: ticket.port,
        username: ticket.username,
        domain: ticket.domain,
        width: ticket.width,
        height: ticket.height,
      ),
      certificate: RdpCertificate(
        address: ticket.address,
        port: ticket.port,
        sha256: ticket.fingerprint,
        snapshotStamp: ticket.snapshotStamp,
      ),
      hasSavedPassword: ticket.hasSavedPassword,
    );
  });

  @override
  Future<RdpSessionInfo> connectSavedHost(
    RdpSavedHostTicket ticket, {
    Uint8List? passwordBytes,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) => _guard(() async {
    final bytes = passwordBytes == null ? null : Uint8List.fromList(passwordBytes);
    try {
      final session = await saved.rdpSavedHostConnect(
        ticket: saved.RdpSavedHostTicket(
          hostId: ticket.hostId,
          name: ticket.name,
          address: ticket.options.address,
          port: ticket.options.port,
          username: ticket.options.username,
          domain: ticket.options.domain,
          width: ticket.options.width,
          height: ticket.options.height,
          fingerprint: ticket.certificate.sha256,
          snapshotStamp: ticket.certificate.snapshotStamp!,
          hasSavedPassword: ticket.hasSavedPassword,
        ),
        password: bytes,
        permissions: _permissions(permissions),
      );
      return RdpSessionInfo(id: session.id, width: session.width, height: session.height);
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
    }
  });

  @override
  Future<RdpCapabilities> capabilities() => _guard(() async {
    final caps = await native.rdpCapabilities();
    return RdpCapabilities(clipboardSupported: caps.clipboardSupported, folderSupported: caps.folderSupported);
  });
  @override
  Future<RdpDirectoryGrant?> pickDirectory() => _guard(() async {
    final grant = await native.rdpPickDirectory();
    return grant == null ? null : RdpDirectoryGrant(id: grant.id, name: grant.name);
  });
  @override
  Future<void> releaseDirectoryGrant(String grantId) => _guard(() => native.rdpReleaseDirectoryGrant(grantId: grantId));
  @override
  Future<RdpSessionPermissions> permissions(String sessionId) => _guard(() async {
    final flags = await native.rdpPermissions(sessionId: sessionId);
    return RdpSessionPermissions(
      clipboardEnabled: flags.clipboardEnabled,
      directoryGrantId: flags.directoryGrantId,
      directoryWritable: flags.directoryWritable,
    );
  });
  @override
  Future<void> setPermissions(String sessionId, RdpSessionPermissions permissions) =>
      _guard(() => native.rdpSetPermissions(sessionId: sessionId, permissions: _permissions(permissions)));
  @override
  Future<void> offerClipboardText(String sessionId, String text) =>
      _guard(() => native.rdpOfferClipboardText(sessionId: sessionId, text: text));
  @override
  Future<void> requestClipboardText(String sessionId) =>
      _guard(() => native.rdpRequestClipboardText(sessionId: sessionId));
  @override
  Future<String?> takeClipboardText(String sessionId) =>
      _guard(() => native.rdpTakeClipboardText(sessionId: sessionId));

  @override
  Future<RdpPollResult> pollFrame(String sessionId) => _guard(() async {
    final result = await native.rdpPoll(sessionId: sessionId);
    final phase = switch (result.phase) {
      'connecting' => RdpPhase.connecting,
      'connected' => RdpPhase.connected,
      'disconnected' => RdpPhase.disconnected,
      _ => RdpPhase.failed,
    };
    final frame = result.frame;
    return RdpPollResult(
      status: RdpStatus(phase, errorCode: result.errorCode),
      folderState: switch (result.folderStatus) {
        'disabled' => RdpFolderState.disabled,
        'pending' => RdpFolderState.pending,
        'ready' => RdpFolderState.ready,
        'denied' => RdpFolderState.denied,
        _ => RdpFolderState.unavailable,
      },
      frame: frame == null
          ? null
          : RdpFrame(sequence: frame.sequence, width: frame.width, height: frame.height, rgba: frame.rgba),
    );
  });

  native.RdpInputMessage _input(RdpInput event) {
    var kind = '';
    var text = '';
    var code = 0;
    var down = false;
    var extended = false;
    var x = 0;
    var y = 0;
    var button = '';
    var vertical = 0;
    var horizontal = 0;
    var width = 0;
    var height = 0;
    switch (event) {
      case RdpUnicodeInput():
        kind = 'unicode';
        text = event.text;
      case RdpScancodeInput():
        kind = 'scancode';
        code = event.code;
        down = event.down;
        extended = event.extended;
      case RdpPointerInput():
        kind = 'pointer';
        x = event.x;
        y = event.y;
        button = event.button?.name ?? '';
        down = event.down;
      case RdpWheelInput():
        kind = 'wheel';
        x = event.x;
        y = event.y;
        // RDP wheel rotation has a signed nine-bit magnitude. A large
        // trackpad gesture must not fail the whole remote session.
        vertical = event.vertical.clamp(-255, 255).toInt();
        horizontal = event.horizontal.clamp(-255, 255).toInt();
      case RdpResizeInput():
        kind = 'resize';
        width = event.width;
        height = event.height;
      case RdpReleaseAllInput():
        kind = 'release_all';
    }
    return native.RdpInputMessage(
      kind: kind,
      text: text,
      code: code,
      down: down,
      extended: extended,
      x: x,
      y: y,
      button: button,
      vertical: vertical,
      horizontal: horizontal,
      width: width,
      height: height,
    );
  }

  @override
  Future<void> sendInput(String sessionId, List<RdpInput> inputs) => _guard(() async {
    if (inputs.length > 128) throw const RdpFailure('input_queue_full');
    await native.rdpSendInput(sessionId: sessionId, messages: inputs.map(_input).toList(growable: false));
  });

  @override
  Future<void> disconnect(String sessionId) => _guard(() => native.rdpDisconnect(sessionId: sessionId));
}
