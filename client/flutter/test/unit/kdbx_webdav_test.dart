import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/webdav.dart';
import 'package:flutter_test/flutter_test.dart';

class _RealHttp extends HttpOverrides {}

void main() {
  void realTest(String name, Future<void> Function() body) =>
      test(name, () => HttpOverrides.runWithHttpOverrides(body, _RealHttp()));
  realTest('WebDAV uses authenticated GET and conditional PUT, and preserves a conflict', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var content = Uint8List.fromList([1, 2, 3]);
    var etag = '"v1"';
    final seen = <String>[];
    server.listen((request) async {
      seen.add(request.method);
      expect(
        request.headers.value(HttpHeaders.authorizationHeader),
        'Basic ${base64.encode(utf8.encode('user:token'))}',
      );
      if (request.method == 'PUT') {
        if (request.headers.value(HttpHeaders.ifMatchHeader) != etag) {
          request.response.statusCode = 412;
        } else {
          final chunks = await request.fold<List<int>>([], (all, chunk) => all..addAll(chunk));
          content = Uint8List.fromList(chunks);
          etag = '"v2"';
          request.response.statusCode = 204;
        }
      }
      request.response.headers.set(HttpHeaders.etagHeader, etag);
      if (request.method == 'GET') request.response.add(content);
      await request.response.close();
    });
    final dav = KdbxWebDav(
      Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'),
      username: 'user',
      password: 'token',
      allowLoopbackHttp: true,
    );
    final file = await dav.read();
    expect(file.bytes, [1, 2, 3]);
    expect(file.etag, '"v1"');
    await dav.write(Uint8List.fromList([4, 5]), file.etag);
    expect(content, [4, 5]);
    await expectLater(
      dav.write(Uint8List.fromList([9]), file.etag),
      throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'changed')),
    );
    expect(content, [4, 5]);
    expect(seen, ['GET', 'PUT', 'PUT']);
    for (final unsafe in [null, 'W/"v2"']) {
      await expectLater(
        dav.write(Uint8List.fromList([9]), unsafe),
        throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_etag')),
      );
    }
  });

  realTest('WebDAV rejects insecure URLs, redirects and unauthorized responses', () async {
    for (final value in [
      'http://example.org/v.kdbx',
      'https://user:pw@example.org/v.kdbx',
      'https://example.org/v.kdbx#token',
    ]) {
      expect(() => KdbxWebDav(Uri.parse(value)), throwsA(isA<KdbxFailure>()));
    }
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var requests = 0;
    server.listen((r) async {
      requests++;
      r.response.statusCode = r.uri.path == '/auth.kdbx' ? 401 : 302;
      r.response.headers.set(HttpHeaders.locationHeader, 'http://127.0.0.1:${server.port}/auth.kdbx');
      await r.response.close();
    });
    final base = 'http://127.0.0.1:${server.port}';
    await expectLater(
      KdbxWebDav(Uri.parse('$base/redirect.kdbx'), allowLoopbackHttp: true).read(),
      throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_redirect')),
    );
    expect(requests, 1);
    await expectLater(
      KdbxWebDav(Uri.parse('$base/auth.kdbx'), allowLoopbackHttp: true).read(),
      throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_auth')),
    );
  });

  realTest('locking cancels an in-flight WebDAV request and closes the transport', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final received = Completer<void>();
    server.listen((request) {
      received.complete();
    });
    final dav = KdbxWebDav(Uri.parse('http://127.0.0.1:${server.port}/v.kdbx'), allowLoopbackHttp: true);
    final pending = dav.read();
    final rejected = expectLater(pending, throwsA(isA<KdbxFailure>()));
    await received.future;
    dav.close();
    await rejected;
    await expectLater(dav.read(), throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'closed')));
  });
  realTest('server and directory URLs are rejected locally before networking', () async {
    for (final url in [
      'https://webdav.yandex.ru',
      'https://dav.example/',
      'https://dav.example/folder/',
      'https://dav.example/folder',
      'https://dav.example/file.txt',
    ]) {
      expect(() => KdbxWebDav(Uri.parse(url)), throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_url')));
    }
    expect(KdbxWebDav.isFileUri(Uri.parse('https://dav.example/folder/Passwords.KDBX')), isTrue);
  });

  realTest('bare tags and PUT without ETag support repeated saves using DAV metadata', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var content = Uint8List.fromList([1, 2, 3]);
    var revision = 1;
    var returnGetTag = true;
    final conditions = <String>[];
    server.listen((request) async {
      final condition = request.headers.value(HttpHeaders.ifMatchHeader);
      if (condition != null) {
        conditions.add('${request.method}:$condition');
        if (condition != '"v$revision"') {
          request.response.statusCode = 412;
          await request.response.close();
          return;
        }
      }
      if (request.method == 'GET') {
        if (returnGetTag) request.response.headers.set(HttpHeaders.etagHeader, 'v$revision');
        request.response.add(content);
      } else if (request.method == 'PROPFIND') {
        expect(request.headers.value('Depth'), '0');
        request.response.statusCode = 207;
        request.response.write(
          '<d:multistatus xmlns:d="DAV:"><d:response><d:href> /vault.kdbx </d:href><d:propstat><d:prop><d:getetag>v$revision</d:getetag></d:prop><d:status> HTTP/1.1 200 OK </d:status></d:propstat></d:response></d:multistatus>',
        );
      } else if (request.method == 'PUT') {
        expect(condition, isNotNull);
        content = Uint8List.fromList(await request.fold<List<int>>([], (all, chunk) => all..addAll(chunk)));
        revision++;
        returnGetTag = false;
        request.response.statusCode = 204;
      }
      await request.response.close();
    });
    final dav = KdbxWebDav(Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'), allowLoopbackHttp: true);
    addTearDown(dav.close);
    final first = await dav.read();
    expect(first.etag, '"v1"');
    final second = await dav.write(Uint8List.fromList([4, 5]), first.etag);
    expect(second.etag, '"v2"');
    final third = await dav.write(Uint8List.fromList([6, 7]), second.etag);
    expect(third.etag, '"v3"');
    expect(content, [6, 7]);
    expect(conditions, containsAllInOrder(['PUT:"v1"', 'GET:"v2"', 'PUT:"v2"', 'GET:"v3"']));
  });

  realTest('property ETag is bound by conditional GET to the bytes actually opened', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var content = <int>[1];
    var revision = 1;
    var gets = 0;
    server.listen((r) async {
      if (r.method == 'GET') {
        gets++;
        if (r.headers.value(HttpHeaders.ifMatchHeader) != null &&
            r.headers.value(HttpHeaders.ifMatchHeader) != '"v$revision"') {
          r.response.statusCode = 412;
        } else {
          r.response.add(content);
        }
      } else if (r.method == 'PROPFIND') {
        content = [2];
        revision = 2;
        r.response.statusCode = 207;
        r.response.write(
          '<multistatus xmlns="DAV:"><response><href>https://attacker.invalid/vault.kdbx</href><propstat><prop><getetag>"wrong"</getetag></prop><status>HTTP/1.1 200 OK</status></propstat></response><response><href>/vault.kdbx</href><propstat><prop><getetag>"v2"</getetag></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>',
        );
      } else if (r.method == 'PUT') {
        r.response.statusCode = 412;
      }
      await r.response.close();
    });
    final dav = KdbxWebDav(Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'), allowLoopbackHttp: true);
    addTearDown(dav.close);
    final file = await dav.read();
    expect(file.bytes, [2]);
    expect(file.etag, '"v2"');
    expect(gets, 2);
    revision = 3;
    content = [3];
    await expectLater(
      dav.write(Uint8List.fromList([4]), file.etag),
      throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'changed')),
    );
    expect(content, [3]);
  });

  for (final weak in [false, true]) {
    realTest('no strong ETag (weak=$weak) uses exclusive lock, detects conflict and unlocks', () async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      var content = <int>[1, 2];
      var locked = false;
      var puts = 0;
      var unlocks = 0;
      var ownedByOther = false;
      const token = '<urn:uuid:consolecrypt-test-lock>';
      server.listen((r) async {
        switch (r.method) {
          case 'GET':
            if (locked) expect(r.headers.value('If'), '($token)');
            if (weak) r.response.headers.set(HttpHeaders.etagHeader, 'W/"weak"');
            r.response.add(content);
          case 'PROPFIND':
            r.response.statusCode = 405;
          case 'HEAD':
            if (weak) r.response.headers.set(HttpHeaders.etagHeader, 'W/"weak"');
          case 'LOCK':
            if (ownedByOther) {
              r.response.statusCode = 423;
              break;
            }
            expect(r.headers.value(HttpHeaders.ifMatchHeader), '*');
            expect(utf8.decode(await r.fold<List<int>>([], (all, chunk) => all..addAll(chunk))), contains('exclusive'));
            locked = true;
            r.response.headers.set('Lock-Token', token);
            r.response.headers.set('Timeout', 'Second-120');
          case 'PUT':
            expect(locked, isTrue);
            expect(r.headers.value('If'), '($token)');
            content = await r.fold<List<int>>([], (all, chunk) => all..addAll(chunk));
            puts++;
            r.response.statusCode = 204;
          case 'UNLOCK':
            expect(r.headers.value('Lock-Token'), token);
            locked = false;
            unlocks++;
            r.response.statusCode = 204;
        }
        await r.response.close();
      });
      final dav = KdbxWebDav(Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'), allowLoopbackHttp: true);
      addTearDown(dav.close);
      final opened = await dav.read();
      expect(opened.etag, isNull);
      final saved = await dav.write(Uint8List.fromList([3, 4]), opened.etag, fingerprint: opened.fingerprint);
      expect(content, [3, 4]);
      expect(puts, 1);
      expect(locked, isFalse);
      expect(unlocks, 1);
      content = [9];
      await expectLater(
        dav.write(Uint8List.fromList([5]), saved.etag, fingerprint: saved.fingerprint),
        throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'changed')),
      );
      expect(content, [9]);
      expect(puts, 1);
      expect(locked, isFalse);
      expect(unlocks, 2);
      ownedByOther = true;
      await expectLater(
        dav.write(Uint8List.fromList([5]), saved.etag, fingerprint: saved.fingerprint),
        throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_locked')),
      );
      expect(puts, 1);
    });
  }

  realTest('unsupported or too short locks never cause an unconditional overwrite', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    var unsupported = true;
    var puts = 0;
    var unlocks = 0;
    server.listen((r) async {
      switch (r.method) {
        case 'GET':
          r.response.add([1]);
        case 'PROPFIND':
          r.response.statusCode = 405;
        case 'LOCK':
          if (unsupported) {
            r.response.statusCode = 405;
          } else {
            r.response.headers.set('Lock-Token', '<urn:uuid:test>');
            r.response.headers.set('Timeout', 'Second-1');
          }
        case 'PUT':
          puts++;
        case 'UNLOCK':
          unlocks++;
          r.response.statusCode = 204;
      }
      await r.response.close();
    });
    final dav = KdbxWebDav(Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'), allowLoopbackHttp: true);
    addTearDown(dav.close);
    final file = await dav.read();
    for (final flag in [true, false]) {
      unsupported = flag;
      await expectLater(
        dav.write(Uint8List.fromList([2]), null, fingerprint: file.fingerprint),
        throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_etag')),
      );
    }
    expect(puts, 0);
    expect(unlocks, 1);
  });

  realTest('an unresponsive server times out without blocking the event loop', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((r) {});
    final dav = KdbxWebDav(
      Uri.parse('http://127.0.0.1:${server.port}/vault.kdbx'),
      allowLoopbackHttp: true,
      requestTimeout: const Duration(milliseconds: 100),
    );
    addTearDown(dav.close);
    var responsive = false;
    Timer.run(() => responsive = true);
    final time = Stopwatch()..start();
    await expectLater(dav.read(), throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'dav_timeout')));
    expect(responsive, isTrue);
    expect(time.elapsed, lessThan(const Duration(seconds: 2)), reason: 'bounded timeout');
  });
}
