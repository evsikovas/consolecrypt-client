import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_riverpod/flutter_riverpod.dart';

final knownHostsImportServiceProvider = Provider<KnownHostsImportService>((_) => KnownHostsImportService());

class KnownHostCandidate {
  const KnownHostCandidate(this.address, this.port);
  final String address;
  final int port;
  String get identity => '${address.toLowerCase()}\u0000$port';
  String get label => port == 22 ? address : (address.contains(':') ? '[$address]:$port' : '$address:$port');
}

class KnownHostsScan {
  const KnownHostsScan({this.hosts = const [], this.sources = const [], this.unreadable = const [], this.skipped = 0});
  final List<KnownHostCandidate> hosts;
  final List<String> sources, unreadable;
  final int skipped;
}

/// Reads only the standard OpenSSH host-key files on explicit user request.
/// This builds connection candidates, never trusted keys or credentials.
class KnownHostsImportService {
  static const maxBytes = 4 * 1024 * 1024;

  List<String> paths() {
    if (Platform.isWindows) {
      final home = Platform.environment['USERPROFILE'];
      return home == null || home.isEmpty ? [] : ['$home\\.ssh\\known_hosts']; // l10n-ignore: Windows OpenSSH path
    }
    if (Platform.isMacOS || Platform.isLinux) {
      final home = Platform.environment['HOME'];
      return [if (home != null && home.isNotEmpty) '$home/.ssh/known_hosts', '/etc/ssh/ssh_known_hosts'];
    }
    return [];
  }

  Future<KnownHostsScan> scan() async {
    final hosts = <String, KnownHostCandidate>{};
    final sources = <String>[], unreadable = <String>[];
    var skipped = 0;
    for (final path in paths()) {
      RandomAccessFile? handle;
      try {
        // A user's .ssh directory can be on a network mount; keep the UI free.
        // ignore: avoid_slow_async_io
        final type = await FileSystemEntity.type(path);
        if (type == FileSystemEntityType.notFound) continue;
        if (type != FileSystemEntityType.file) {
          unreadable.add(path);
          continue;
        }
        // Opening raises permission failures rather than reporting them as an
        // absent file. The bounded read also handles growth after length().
        handle = await File(path).open();
        if (await handle.length() > maxBytes) {
          unreadable.add(path);
          continue;
        }
        final bytes = await handle.read(maxBytes + 1);
        if (bytes.length > maxBytes) {
          unreadable.add(path);
          continue;
        }
        final parsed = parse(utf8.decode(bytes));
        sources.add(path);
        skipped += parsed.skipped;
        for (final host in parsed.hosts) {
          hosts[host.identity] = host;
        }
      } on FileSystemException catch (error) {
        if (error.osError?.errorCode != 2 && error.osError?.errorCode != 3) unreadable.add(path);
      } on FormatException {
        unreadable.add(path);
      } finally {
        await handle?.close();
      }
    }
    final sorted = hosts.values.toList()..sort((a, b) => a.label.toLowerCase().compareTo(b.label.toLowerCase()));
    return KnownHostsScan(hosts: sorted, sources: sources, unreadable: unreadable, skipped: skipped);
  }

  static KnownHostsScan parse(String text) {
    if (text.length > maxBytes) throw const FormatException('known_hosts_too_large');
    final hosts = <String, KnownHostCandidate>{};
    var skipped = 0;
    for (final line in const LineSplitter().convert(text)) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || trimmed.startsWith('#')) continue;
      final fields = trimmed.split(RegExp(r'\s+'));
      // A CA pattern or revoked key is not an ordinary saved connection.
      if (fields.length < 3 || fields.first.startsWith('@') || !_validKey(fields[1], fields[2])) {
        skipped++;
        continue;
      }
      for (final name in fields.first.split(',')) {
        final host = _host(name);
        if (host == null) {
          skipped++;
        } else {
          hosts[host.identity] = host;
        }
      }
    }
    return KnownHostsScan(hosts: hosts.values.toList(), skipped: skipped);
  }

  static bool _validKey(String algorithm, String encoded) {
    if (encoded.length > 24 * 1024 || !RegExp(r'^(ssh-|ecdsa-|sk-)[A-Za-z0-9@._+-]+$').hasMatch(algorithm)) {
      return false;
    }
    try {
      final bytes = base64.decode(base64.normalize(encoded));
      if (bytes.length < 5) return false;
      final size = ByteData.sublistView(bytes).getUint32(0);
      return size == algorithm.length &&
          size + 4 < bytes.length &&
          ascii.decode(bytes.sublist(4, 4 + size)) == algorithm;
    } on FormatException {
      return false;
    }
  }

  static KnownHostCandidate? _host(String pattern) {
    if (pattern.isEmpty || pattern.length > 270 || RegExp(r'[|!*?\s/#\\]').hasMatch(pattern)) return null;
    var address = pattern;
    var port = 22;
    if (pattern.startsWith('[')) {
      final match = RegExp(r'^\[([^\[\]]+)\]:(\d{1,5})$').firstMatch(pattern);
      if (match == null) return null;
      address = match[1]!;
      port = int.parse(match[2]!);
    }
    if (port < 1 || port > 65535 || address.length > 253) return null;
    if (address.contains(':')) {
      final ip = InternetAddress.tryParse(address);
      if (ip == null || ip.type != InternetAddressType.IPv6) return null;
      address = ip.address;
    } else if (!RegExp(r'^[A-Za-z0-9_](?:[A-Za-z0-9_.-]*[A-Za-z0-9_.])?$').hasMatch(address)) {
      return null;
    }
    return KnownHostCandidate(address, port);
  }
}
