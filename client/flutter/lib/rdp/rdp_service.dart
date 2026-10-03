import 'dart:typed_data';

const rdpMaxWidth = 4096;
const rdpMaxHeight = 2160;
const rdpMaxFrameBytes = 36 * 1024 * 1024;
const rdpMaxSessions = 4;

/// Direct-connection metadata. Credentials are deliberately not part of it.
final class RdpConnectionOptions {
  RdpConnectionOptions({
    required this.address,
    required this.username,
    this.port = 3389,
    this.domain = '',
    this.width = 1280,
    this.height = 720,
  }) {
    if (address.isEmpty ||
        address.length > 253 ||
        RegExp(r'[\s/\x00]').hasMatch(address) ||
        port < 1 ||
        port > 65535 ||
        username.isEmpty ||
        username.length > 256 ||
        domain.length > 256 ||
        RegExp(r'[\x00\r\n]').hasMatch('$username$domain') ||
        width < 200 ||
        width > rdpMaxWidth ||
        width.isOdd ||
        height < 200 ||
        height > rdpMaxHeight) {
      throw const FormatException('invalid_rdp_options');
    }
  }

  final String address;
  final int port;
  final String username;
  final String domain;
  final int width;
  final int height;

  String get endpoint => address.contains(':') ? '[$address]:$port' : '$address:$port';
}

/// A native, pre-authentication probe result. No password is used by the probe.
final class RdpCertificate {
  RdpCertificate({required this.address, required this.port, required String sha256, this.snapshotStamp})
    : sha256 = sha256.toLowerCase() {
    if (address.isEmpty || port < 1 || port > 65535 || !RegExp(r'^[0-9a-f]{64}$').hasMatch(this.sha256)) {
      throw const FormatException('invalid_rdp_certificate');
    }
  }
  final String address;
  final int port;
  final String sha256;

  /// Opaque native saved-host snapshot. Never generated or interpreted by UI.
  final String? snapshotStamp;

  String get displaySha256 => [for (var i = 0; i < sha256.length; i += 2) sha256.substring(i, i + 2)].join(':');
}

final class RdpSessionPermissions {
  const RdpSessionPermissions({this.clipboardEnabled = false, this.directoryGrantId, this.directoryWritable = false});
  final bool clipboardEnabled;
  final String? directoryGrantId;
  final bool directoryWritable;

  RdpSessionPermissions copyWith({
    bool? clipboardEnabled,
    String? directoryGrantId,
    bool? directoryWritable,
    bool clearDirectory = false,
  }) => RdpSessionPermissions(
    clipboardEnabled: clipboardEnabled ?? this.clipboardEnabled,
    directoryGrantId: clearDirectory ? null : directoryGrantId ?? this.directoryGrantId,
    directoryWritable: clearDirectory ? false : directoryWritable ?? this.directoryWritable,
  );
}

/// Native capability, scoped to the current unlocked manager. No path crosses
/// this boundary; the UI cannot request an arbitrary directory by string.
final class RdpDirectoryGrant {
  const RdpDirectoryGrant({required this.id, required this.name});
  final String id;
  final String name;
}

final class RdpCapabilities {
  const RdpCapabilities({required this.clipboardSupported, required this.folderSupported});
  final bool clipboardSupported, folderSupported;
}

/// Platform support above does not imply that a remote server accepted the
/// selected drive. This state is reported by the native RDPDR handshake.
enum RdpFolderState { disabled, pending, ready, denied, unavailable }

final class RdpSavedHostTicket {
  RdpSavedHostTicket({
    required this.hostId,
    required this.name,
    required this.options,
    required this.certificate,
    required this.hasSavedPassword,
  }) {
    if (hostId.isEmpty ||
        certificate.snapshotStamp?.isNotEmpty != true ||
        options.address != certificate.address ||
        options.port != certificate.port) {
      throw const FormatException('invalid_rdp_saved_ticket');
    }
  }
  final String hostId, name;
  final RdpConnectionOptions options;
  final RdpCertificate certificate;
  final bool hasSavedPassword;
}

enum RdpPhase { connecting, connected, disconnected, failed }

final class RdpStatus {
  const RdpStatus(this.phase, {this.errorCode});
  final RdpPhase phase;

  /// A machine-readable code; raw transport errors must not reach the UI.
  final String? errorCode;
}

/// Immutable, bounded RGBA bytes. The incoming transport buffer is copied so
/// a subsequent native poll cannot mutate the frame being decoded by Flutter.
final class RdpFrame {
  RdpFrame({required this.sequence, required this.width, required this.height, required Uint8List rgba}) {
    if (sequence < 0 ||
        width < 1 ||
        width > rdpMaxWidth ||
        height < 1 ||
        height > rdpMaxHeight ||
        rgba.length > rdpMaxFrameBytes ||
        rgba.length != width * height * 4) {
      throw const FormatException('invalid_rdp_frame');
    }
    _rgba = Uint8List.fromList(rgba).asUnmodifiableView();
  }
  final int sequence;
  final int width;
  final int height;
  late final Uint8List _rgba;
  Uint8List get rgba => _rgba;
}

final class RdpSessionInfo {
  RdpSessionInfo({required this.id, required this.width, required this.height}) {
    if (id.isEmpty || id.length > 128 || width < 1 || width > rdpMaxWidth || height < 1 || height > rdpMaxHeight) {
      throw const FormatException('invalid_rdp_session');
    }
  }
  final String id;
  final int width;
  final int height;
}

final class RdpPollResult {
  const RdpPollResult({required this.status, this.frame, this.folderState = RdpFolderState.disabled});
  final RdpStatus status;
  final RdpFrame? frame;
  final RdpFolderState folderState;
}

sealed class RdpInput {
  const RdpInput();
}

/// Native release-all also clears any input state unknown to this view.
final class RdpReleaseAllInput extends RdpInput {
  const RdpReleaseAllInput();
}

final class RdpUnicodeInput extends RdpInput {
  const RdpUnicodeInput(this.text);
  final String text;
  @override
  String toString() => 'RdpUnicodeInput(<redacted>)';
}

final class RdpScancodeInput extends RdpInput {
  const RdpScancodeInput(this.code, {required this.down, this.extended = false});
  final int code;
  final bool down;
  final bool extended;
}

enum RdpPointerButton { left, middle, right }

final class RdpPointerInput extends RdpInput {
  const RdpPointerInput(this.x, this.y, {this.button, this.down = false});
  final int x;
  final int y;
  final RdpPointerButton? button;
  final bool down;
}

final class RdpWheelInput extends RdpInput {
  const RdpWheelInput(this.x, this.y, this.vertical, {this.horizontal = 0});
  final int x;
  final int y;
  final int vertical;
  final int horizontal;
}

final class RdpResizeInput extends RdpInput {
  const RdpResizeInput(this.width, this.height);
  final int width;
  final int height;
}

abstract interface class RdpService {
  Future<RdpCertificate> probeCertificate(String address, int port);

  /// The caller owns [passwordBytes] until this Future settles, then wipes it.
  /// Implementations must copy/zeroize at the native boundary and verify the
  /// exact certificate pin again before authenticating. No redirect fallback.
  Future<RdpSessionInfo> connect(
    RdpConnectionOptions options, {
    required Uint8List passwordBytes,
    required String certificateSha256,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  });
  Future<RdpSavedHostTicket> probeSavedHost(String hostId);
  Future<RdpSessionInfo> connectSavedHost(
    RdpSavedHostTicket ticket, {
    Uint8List? passwordBytes,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  });
  Future<RdpCapabilities> capabilities();
  Future<RdpDirectoryGrant?> pickDirectory();
  Future<void> releaseDirectoryGrant(String grantId);
  Future<RdpSessionPermissions> permissions(String sessionId);
  Future<void> setPermissions(String sessionId, RdpSessionPermissions permissions);
  Future<void> offerClipboardText(String sessionId, String text);

  /// Returns a one-use opaque ticket only after the current format list is
  /// acknowledged. A caller must recheck its UI origin before committing it.
  Future<String> offerClipboardTextConfirmed(String sessionId, String text);

  /// Native dispatch rechecks the ticket, offer and permission generation.
  Future<void> commitClipboardPaste(String sessionId, String ticket);
  Future<void> requestClipboardText(String sessionId);
  Future<String?> takeClipboardText(String sessionId);
  Future<RdpPollResult> pollFrame(String sessionId);
  Future<void> sendInput(String sessionId, List<RdpInput> inputs);
  Future<void> disconnect(String sessionId);
}

final class UnavailableRdpService implements RdpService {
  const UnavailableRdpService();
  @override
  Future<RdpCertificate> probeCertificate(String address, int port) async => throw const RdpFailure('unavailable');
  @override
  Future<RdpSessionInfo> connect(
    RdpConnectionOptions options, {
    required Uint8List passwordBytes,
    required String certificateSha256,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) async => throw const RdpFailure('unavailable');
  @override
  Future<RdpSavedHostTicket> probeSavedHost(String hostId) async => throw const RdpFailure('unavailable');
  @override
  Future<RdpSessionInfo> connectSavedHost(
    RdpSavedHostTicket ticket, {
    Uint8List? passwordBytes,
    RdpSessionPermissions permissions = const RdpSessionPermissions(),
  }) async => throw const RdpFailure('unavailable');
  @override
  Future<RdpCapabilities> capabilities() async =>
      const RdpCapabilities(clipboardSupported: false, folderSupported: false);
  @override
  Future<RdpDirectoryGrant?> pickDirectory() async => throw const RdpFailure('unavailable');
  @override
  Future<void> releaseDirectoryGrant(String grantId) async {}
  @override
  Future<RdpSessionPermissions> permissions(String sessionId) async => const RdpSessionPermissions();
  @override
  Future<void> setPermissions(String sessionId, RdpSessionPermissions permissions) async =>
      throw const RdpFailure('unavailable');
  @override
  Future<void> offerClipboardText(String sessionId, String text) async => throw const RdpFailure('unavailable');
  @override
  Future<String> offerClipboardTextConfirmed(String sessionId, String text) async =>
      throw const RdpFailure('unavailable');
  @override
  Future<void> commitClipboardPaste(String sessionId, String ticket) async => throw const RdpFailure('unavailable');
  @override
  Future<void> requestClipboardText(String sessionId) async => throw const RdpFailure('unavailable');
  @override
  Future<String?> takeClipboardText(String sessionId) async => throw const RdpFailure('unavailable');
  @override
  Future<RdpPollResult> pollFrame(String sessionId) async => throw const RdpFailure('unavailable');
  @override
  Future<void> sendInput(String sessionId, List<RdpInput> inputs) async => throw const RdpFailure('unavailable');
  @override
  Future<void> disconnect(String sessionId) async {}
}

final class RdpFailure implements Exception {
  const RdpFailure(this.code);
  final String code;
  @override
  String toString() => 'RdpFailure(<redacted>)';
}
