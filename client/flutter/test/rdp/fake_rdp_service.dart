import 'dart:async';
import 'dart:typed_data';

import 'package:consolecrypt/rdp/rdp_service.dart';

final class FakeRdpService implements RdpService {
  final disconnected = <String>[];
  final inputs = <List<RdpInput>>[];
  final inputSessionIds = <String>[];
  final options = <RdpConnectionOptions>[];
  final pins = <String>[];
  Uint8List? borrowedPassword;
  Completer<RdpSessionInfo>? connectGate;
  Completer<RdpCertificate>? probeGate;
  Completer<RdpPollResult>? pollGate;
  Completer<void>? inputGate;
  RdpFailure? inputFailure;
  int pollCount = 0, probeCount = 0;
  int clipboardRequests = 0, clipboardTakes = 0, folderPicks = 0;
  final permissionCalls = <RdpSessionPermissions>[];
  final releasedGrants = <String>[];
  final clipboardOffers = <String>[];
  RdpSessionPermissions currentPermissions = const RdpSessionPermissions();
  RdpCapabilities supported = const RdpCapabilities(clipboardSupported: true, folderSupported: true);
  RdpDirectoryGrant? pickedDirectory;
  RdpSavedHostTicket? savedTicket;
  Completer<RdpSavedHostTicket>? savedProbeGate;
  Completer<RdpDirectoryGrant?>? pickerGate;
  Completer<void>? permissionGate;
  Completer<String?>? clipboardGate;
  RdpFailure? permissionFailure;
  RdpFailure? pickerFailure;
  RdpFailure? clipboardFailure;
  String? remoteClipboard;
  RdpSavedHostTicket? usedTicket;

  RdpPollResult nextPoll = const RdpPollResult(status: RdpStatus(RdpPhase.connected));
  String fingerprint = List.filled(32, 'ab').join();

  @override
  Future<RdpCertificate> probeCertificate(String address, int port) async {
    probeCount++;
    return probeGate?.future ?? RdpCertificate(address: address, port: port, sha256: fingerprint);
  }

  @override
  Future<RdpSessionInfo> connect(
    RdpConnectionOptions value, {
    required Uint8List passwordBytes,
    required String certificateSha256,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) async {
    currentPermissions = permissions;
    options.add(value);
    pins.add(certificateSha256);
    borrowedPassword = passwordBytes;
    return connectGate?.future ?? RdpSessionInfo(id: 'rdp-${options.length}', width: value.width, height: value.height);
  }

  @override
  Future<RdpSavedHostTicket> probeSavedHost(String hostId) async => savedProbeGate?.future ?? savedTicket!;
  @override
  Future<RdpSessionInfo> connectSavedHost(
    RdpSavedHostTicket ticket, {
    Uint8List? passwordBytes,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) async {
    usedTicket = ticket;
    currentPermissions = permissions;
    options.add(ticket.options);
    pins.add(ticket.certificate.sha256);
    borrowedPassword = passwordBytes;
    return connectGate?.future ??
        RdpSessionInfo(id: 'rdp-${options.length}', width: ticket.options.width, height: ticket.options.height);
  }

  @override
  Future<RdpCapabilities> capabilities() async => supported;
  @override
  Future<RdpDirectoryGrant?> pickDirectory() async {
    folderPicks++;
    if (pickerFailure != null) throw pickerFailure!;
    return pickerGate?.future ?? pickedDirectory;
  }

  @override
  Future<void> releaseDirectoryGrant(String grantId) async {
    releasedGrants.add(grantId);
  }

  @override
  Future<RdpSessionPermissions> permissions(String sessionId) async => currentPermissions;
  @override
  Future<void> setPermissions(String sessionId, RdpSessionPermissions permissions) async {
    permissionCalls.add(permissions);
    await permissionGate?.future;
    if (permissionFailure != null) throw permissionFailure!;
    currentPermissions = permissions;
  }

  @override
  Future<void> offerClipboardText(String sessionId, String text) async {
    clipboardOffers.add(text);
  }

  @override
  Future<void> requestClipboardText(String sessionId) async {
    clipboardRequests++;
    if (clipboardFailure != null) throw clipboardFailure!;
  }

  @override
  Future<String?> takeClipboardText(String sessionId) async {
    clipboardTakes++;
    return clipboardGate?.future ?? remoteClipboard;
  }

  @override
  Future<RdpPollResult> pollFrame(String sessionId) async {
    pollCount++;
    return pollGate?.future ?? nextPoll;
  }

  @override
  Future<void> sendInput(String sessionId, List<RdpInput> batch) async {
    inputs.add(batch);
    inputSessionIds.add(sessionId);
    if (inputFailure != null) throw inputFailure!;
    await inputGate?.future;
  }

  @override
  Future<void> disconnect(String sessionId) async {
    disconnected.add(sessionId);
  }
}
