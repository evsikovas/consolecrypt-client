import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/local_auth_channel.dart';
import 'package:consolecrypt/keepass/kdbx_android_auth.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

Uint8List handle(String alias) =>
    Uint8List.fromList([1, alias.length, ...utf8.encode(alias), 12, ...List.filled(60, 9)]);
const oldAlias = 'cc-device-unlock-kdbx-00000000-0000-4000-8000-000000000001';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('KDBX enrollment and unlock can wipe material from read-only Android replies', () async {
    const channel = 'consolecrypt/local_auth';
    const codec = StandardMethodCodec();
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    final handleBytes = handle(oldAlias);
    final composite = Uint8List.fromList(List.filled(32, 77));
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    addTearDown(() {
      debugDefaultTargetPlatformOverride = null;
      messenger.setMockMessageHandler(channel, null);
    });
    messenger.setMockMessageHandler(channel, (message) async {
      final request = codec.decodeMethodCall(message!);
      final material = Uint8List.fromList(List.filled(32, 23));
      // The Android engine delivers an unmodifiable ByteData to the codec.
      return codec
          .encodeSuccessEnvelope(switch (request.method) {
            'createKey' => {'material': material, 'handle': handleBytes},
            'releaseKey' => material,
            _ => throw StateError('Unexpected call: ${request.method}'),
          })
          .asUnmodifiableView();
    });

    final auth = KdbxAndroidAuth();
    Uint8List? releasedMaterial;
    Future<Uint8List> call(String action, Uint8List bytes) async {
      switch (action) {
        case 'enable_external':
          expect(bytes.sublist(0, 32), composite);
          expect(bytes.sublist(32, 64), List.filled(32, 23));
          return Uint8List(0);
        case 'external_handle':
          return handleBytes;
        case 'release_external':
          expect(bytes, List.filled(32, 23));
          releasedMaterial = bytes;
          return composite;
        default:
          throw StateError('Unexpected action: $action');
      }
    }

    await auth.protect(call, composite, 'Unlock', enabled: false);
    expect(await auth.unlock(call, 'Unlock'), composite);
    expect(releasedMaterial, List.filled(32, 0));
  });

  test('an invalidated Android key retires only its handle and asks for the master password', () async {
    final androidHandle = handle(oldAlias);
    final deleted = <String>[];
    final auth = KdbxAndroidAuth(
      release: (_, _) async => throw const LocalAuthInvalidated(),
      delete: (alias) async => deleted.add(alias),
    );
    final calls = <String>[];
    await expectLater(
      auth.unlock((action, bytes) async {
        calls.add(action);
        if (action == 'external_handle') return androidHandle;
        expect(bytes, androidHandle);
        return Uint8List(0);
      }, 'Unlock'),
      throwsA(isA<KdbxFailure>().having((error) => error.code, 'code', 'device_key_invalidated')),
    );
    expect(calls, ['external_handle', 'forget_external_handle']);
    expect(deleted, [oldAlias]);
  });

  test('Android enrollment replaces the old alias only after core commit and wipes payload', () async {
    final material = Uint8List.fromList(List.filled(32, 23));
    final composite = Uint8List.fromList(List.filled(32, 77));
    final events = <String>[];
    Uint8List? boundary;
    final auth = KdbxAndroidAuth(
      create: (alias, reason) async {
        events.add('create');
        return LocalAuthKey(material: material, handle: handle(alias));
      },
      delete: (alias) async => events.add('delete:$alias'),
    );
    await auth.protect(
      (action, bytes) async {
        if (action == 'external_handle') return handle(oldAlias);
        events.add(action);
        boundary = bytes;
        expect(bytes.take(32), composite);
        expect(bytes.sublist(32, 64), material);
        return Uint8List(0);
      },
      composite,
      'Unlock',
      enabled: true,
    );
    expect(events, ['create', 'enable_external', 'delete:$oldAlias']);
    expect(material.every((b) => b == 0), isTrue);
    expect(boundary!.every((b) => b == 0), isTrue);
    expect(composite.every((b) => b == 77), isTrue); // Caller still owns it.
  });

  test('failed enrollment keeps the previous alias and deletes the new one', () async {
    final material = Uint8List.fromList(List.filled(32, 23));
    String? created;
    final deleted = <String>[];
    final auth = KdbxAndroidAuth(
      create: (alias, _) async {
        created = alias;
        return LocalAuthKey(material: material, handle: handle(alias));
      },
      delete: (alias) async => deleted.add(alias),
    );
    await expectLater(
      auth.protect(
        (action, bytes) async {
          if (action == 'external_handle') return handle(oldAlias);
          throw StateError('synthetic keyfile write failure');
        },
        Uint8List(32),
        'Unlock',
        enabled: true,
      ),
      throwsStateError,
    );
    expect(deleted, [created]);
    expect(deleted, isNot(contains(oldAlias)));
    expect(material.every((b) => b == 0), isTrue);
  });

  test('unlock wipes material on success and wrong-key rejection; forgetting never creates a key', () async {
    for (final reject in [false, true]) {
      final material = Uint8List.fromList(List.filled(32, 23));
      final auth = KdbxAndroidAuth(
        release: (bytes, _) async {
          expect(bytes, handle(oldAlias));
          return material;
        },
      );
      final future = auth.unlock((action, bytes) async {
        if (action == 'external_handle') return handle(oldAlias);
        expect(action, 'release_external');
        if (reject) throw StateError('synthetic wrong OS key');
        return Uint8List.fromList(List.filled(32, 77));
      }, 'Unlock');
      if (reject) {
        await expectLater(future, throwsStateError);
      } else {
        expect(await future, List.filled(32, 77));
      }
      expect(material.every((b) => b == 0), isTrue);
    }
    final events = <String>[];
    final auth = KdbxAndroidAuth(
      create: (_, _) => throw StateError('must not create'),
      delete: (alias) async => events.add('delete:$alias'),
    );
    await auth.protect(
      (action, _) async {
        events.add(action);
        return action == 'external_handle' ? handle(oldAlias) : Uint8List(0);
      },
      Uint8List(0),
      'Unlock',
      enabled: true,
      forget: true,
    );
    expect(events, ['external_handle', 'forget', 'delete:$oldAlias']);
    expect(KdbxAndroidAuth.alias(handle('cc-device-unlock-a-user-profile')), isNull);
  });
}
