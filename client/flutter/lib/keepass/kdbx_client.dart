// Filesystem operations run in the worker, away from the UI isolate.
// ignore_for_file: avoid_slow_async_io

import 'dart:async';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:crypto/crypto.dart';
import 'package:kdbx/kdbx.dart';

typedef KdbxData = Map<String, Object?>;

final class KdbxFailure implements Exception {
  const KdbxFailure(this.code);
  final String code;
  @override
  String toString() => 'KdbxFailure($code)';
}

/// One local file in a disposable isolate. Lists contain metadata only; secret
/// fields cross the boundary only for an explicit edit/copy. No network calls.
class KdbxClient {
  KdbxClient._();
  final ReceivePort _responses = ReceivePort();
  final Map<int, Completer<Object?>> _pending = {};
  late final StreamSubscription<Object?> _subscription;
  Isolate? _isolate;
  SendPort? _commands;
  var _serial = 0;
  var _closed = false;

  static Future<KdbxClient> start() async {
    final client = KdbxClient._();
    final ready = Completer<void>();
    client._subscription = client._responses.listen((Object? response) {
      if (response is SendPort) {
        client._commands = response;
        ready.complete();
      } else if (response is (int, Object?, String?)) {
        final completer = client._pending.remove(response.$1);
        if (response.$3 case final code?) {
          completer?.completeError(KdbxFailure(code));
        } else {
          completer?.complete(response.$2);
        }
      }
    });
    try {
      client._isolate = await Isolate.spawn(_worker, client._responses.sendPort);
      await ready.future.timeout(const Duration(seconds: 10));
      return client;
    } catch (_) {
      client.close();
      throw const KdbxFailure('unavailable');
    }
  }

  Future<Object?> request(String action, [KdbxData args = const {}]) async {
    if (_closed) throw const KdbxFailure('closed');
    final id = ++_serial;
    final result = Completer<Object?>();
    _pending[id] = result;
    _commands!.send((id, action, args));
    try {
      return await result.future.timeout(const Duration(minutes: 2));
    } on TimeoutException {
      close();
      throw const KdbxFailure('limits');
    } finally {
      _pending.remove(id);
    }
  }

  Future<KdbxData> open(String path, SecretText password, {String? keyFile, bool create = false}) async {
    final bytes = password.exposeBytes();
    try {
      // SendPort clones the bytes before send returns; wipe this boundary copy.
      final result = request(create ? 'create' : 'open', {'path': path, 'password': bytes, 'keyFile': keyFile});
      bytes.fillRange(0, bytes.length, 0);
      return (await result)! as KdbxData;
    } finally {
      bytes.fillRange(0, bytes.length, 0);
      password.wipe();
    }
  }

  Future<KdbxData> openSource(
    String location, {
    SecretText? password,
    String? keyFile,
    Uint8List? encrypted,
    Uint8List? compositeKey,
  }) async {
    final bytes = password?.exposeBytes() ?? Uint8List(0);
    try {
      final pending = request('open', {
        'path': location,
        'password': bytes,
        'keyFile': keyFile,
        'encrypted': encrypted,
        'compositeKey': compositeKey,
      });
      bytes.fillRange(0, bytes.length, 0);
      return (await pending)! as KdbxData;
    } finally {
      bytes.fillRange(0, bytes.length, 0);
      password?.wipe();
    }
  }

  /// Also cancels KDF work. The worker never writes plaintext temporary files.
  void close() {
    if (_closed) return;
    _closed = true;
    _isolate?.kill(priority: Isolate.immediate);
    _responses.close();
    unawaited(_subscription.cancel());
    for (final completer in _pending.values) {
      completer.completeError(const KdbxFailure('closed'));
    }
    _pending.clear();
  }
}

Future<void> _worker(SendPort output) async {
  // AES KDF stays in this worker too; killing it cancels all cryptographic work.
  KdbxFormat.dartWebWorkaround = true;
  final commands = ReceivePort();
  output.send(commands.sendPort);
  final document = _Document();
  await for (final raw in commands) {
    final (id, action, args) = raw as (int, String, KdbxData);
    try {
      output.send((id, await document.run(action, args), null));
    } catch (error) {
      final code = switch (error) {
        KdbxFailure(:final code) => code,
        KdbxInvalidKeyException() => 'key',
        KdbxUnsupportedException() => 'unsupported',
        FileSystemException() => 'io',
        _ => 'format',
      };
      // Library errors can include XML/user fields: never forward or log them.
      output.send((id, null, code));
    }
  }
}

class _Document {
  KdbxFile? _database;
  String _path = '';
  String? _fingerprint;
  bool _dirty = false;
  bool _remote = false;
  KdbxFile get db => _database ?? (throw const KdbxFailure('closed'));

  Future<Uint8List> _read(String path, int max) async {
    final file = File(path);
    if (await file.length() > max) throw const KdbxFailure('limits');
    final input = await file.open();
    try {
      final bytes = await input.read(max + 1);
      if (bytes.length > max) throw const KdbxFailure('limits');
      return bytes;
    } finally {
      await input.close();
    }
  }

  KdbxGroup _group(String? id) =>
      id == null ? db.body.rootGroup : db.body.rootGroup.getAllGroups().firstWhere((g) => g.uuid.toString() == id);
  KdbxEntry _entry(String id) => db.body.rootGroup.getAllEntries().firstWhere((e) => e.uuid.toString() == id);
  bool _recycledGroup(KdbxGroup group) => group.breadcrumbs.any((g) => g.uuid == db.recycleBin?.uuid);
  KdbxGroup _editableGroup(String id) {
    final group = _tree(db.body.rootGroup).where((g) => g.uuid.toString() == id).firstOrNull;
    if (group == null || group.parent == null || _recycledGroup(group)) throw const KdbxFailure('group');
    return group;
  }

  String _value(KdbxEntry entry, String key) => entry.getString(KdbxKey(key))?.getText() ?? '';

  bool _validIcon(Uint8List data) {
    if (data.length < 24 || data.length > 48 * 1024) return false;
    final png = ByteData.sublistView(data);
    return png.getUint32(0) == 0x89504e47 &&
        png.getUint32(4) == 0x0d0a1a0a &&
        png.getUint32(12) == 0x49484452 &&
        png.getUint32(16) >= 1 &&
        png.getUint32(16) <= 256 &&
        png.getUint32(20) >= 1 &&
        png.getUint32(20) <= 256;
  }

  KdbxCustomIcon? _preparedIcon(KdbxData args) {
    final id = args['icon'];
    final png = args['customIcon'];
    if (id != null && (id is! int || id < 0 || id >= KdbxIcon.values.length) ||
        png != null && (png is! Uint8List || !_validIcon(png)) ||
        id != null && png != null) {
      throw const KdbxFailure('format');
    }
    if (png is! Uint8List) return null;
    final hash = sha256.convert(png);
    for (final icon in db.body.meta.customIcons.values) {
      if (icon.data.length == png.length && sha256.convert(icon.data) == hash) return icon;
    }
    return KdbxCustomIcon(uuid: KdbxUuid.random(), data: png);
  }

  void _applyIcon(KdbxObject object, KdbxData args, KdbxCustomIcon? custom) {
    if (custom != null) {
      object.customIcon = custom;
    } else if (args['icon'] case final int id) {
      object.icon.set(KdbxIcon.values[id], force: true);
      if (object.customIconUuid.get() != null) object.customIconUuid.set(KdbxUuid.NIL);
    }
  }

  String? _customIconId(KdbxObject object) {
    final id = object.customIconUuid.get();
    return id == null || id.isNil || id.uuid.isEmpty ? null : id.uuid;
  }

  Map<String, Uint8List> _icons() {
    final result = <String, Uint8List>{};
    var size = 0;
    for (final icon in db.body.meta.customIcons.values) {
      if (!_validIcon(icon.data)) continue;
      size += icon.data.length;
      if (size > 4 * 1024 * 1024 || result.length >= 1024) break;
      result[icon.uuid.uuid] = icon.data;
    }
    return result;
  }

  Iterable<KdbxGroup> _tree(KdbxGroup group) sync* {
    yield group;
    for (final child in group.groups) {
      yield* _tree(child);
    }
  }

  KdbxData snapshot() => {
    'path': _path,
    'root': db.body.rootGroup.uuid.toString(),
    'recycleBin': db.recycleBin?.uuid.toString(),
    'dirty': _dirty,
    'version': db.header.version.toString(),
    'icons': _icons(),
    'groups': [
      for (final group in _tree(db.body.rootGroup))
        {
          'id': group.uuid.toString(),
          'parent': group.parent?.uuid.toString(),
          'recycled': _recycledGroup(group),
          'containsRecycleBin': db.recycleBin?.isInGroup(group) ?? false,
          'label': group.name.get() ?? '',
          'icon': group.icon.get()?.index,
          'iconId': _customIconId(group),
          'name': group.breadcrumbs.map((g) => g.name.get() ?? '').join(' / '),
        },
    ],
    'entries': [
      for (final entry in db.body.rootGroup.getAllEntries())
        {
          'id': entry.uuid.toString(),
          'group': entry.parent?.uuid.toString(),
          'title': _value(entry, 'Title'),
          'username': _value(entry, 'UserName'),
          'url': _value(entry, 'URL'),
          'notes': _value(entry, 'Notes'),
          'icon': entry.icon.get()?.index,
          'iconId': _customIconId(entry),
          'recycled': entry.parent?.breadcrumbs.any((g) => g.uuid == db.recycleBin?.uuid) ?? false,
          'fields': [for (final field in entry.stringEntries) field.key.key],
        },
    ],
  };

  Future<Object?> run(String action, KdbxData args) async {
    switch (action) {
      case 'open' || 'create':
        final password = args['password']! as Uint8List;
        Uint8List? key;
        try {
          if (args['keyFile'] case final String keyPath) key = await _read(keyPath, 1024 * 1024);
          final compositeKey = args['compositeKey'] as Uint8List?;
          if (compositeKey != null && compositeKey.length != 32) throw const KdbxFailure('key');
          final credentials = compositeKey != null
              ? Credentials.fromHash(Uint8List.fromList(compositeKey))
              : Credentials.composite(
                  password.isEmpty && key != null ? null : ProtectedValue.fromBinary(password),
                  key,
                );
          final path = args['path']! as String;
          KdbxFile next;
          String? fingerprint;
          if (action == 'create') {
            if (await File(path).exists()) throw const KdbxFailure('exists');
            if (password.isEmpty && key == null) throw const KdbxFailure('key');
            next = KdbxFormat().create(credentials, File(path).uri.pathSegments.last);
          } else {
            final bytes = args['encrypted'] as Uint8List? ?? await _read(path, 128 * 1024 * 1024);
            if (bytes.length > 128 * 1024 * 1024) throw const KdbxFailure('limits');
            fingerprint = sha256.convert(bytes).toString();
            next = await KdbxFormat().read(bytes, credentials);
          }
          _database?.dispose();
          _database = next;
          _remote = args['encrypted'] != null;
          _path = _remote
              ? path
              : action == 'open'
              ? await File(path).resolveSymbolicLinks()
              : File(path).absolute.path;
          _fingerprint = fingerprint;
          _dirty = action == 'create';
          if (action == 'create') await _save();
          return snapshot();
        } finally {
          password.fillRange(0, password.length, 0);
          key?.fillRange(0, key.length, 0);
          final inputHash = args['compositeKey'] as Uint8List?;
          inputHash?.fillRange(0, inputHash.length, 0);
        }
      case 'snapshot':
        return snapshot();
      case 'compositeKey':
        return Uint8List.fromList(db.credentials.getHash());
      case 'export':
        return db.save();
      case 'saved':
        _dirty = false;
        return snapshot();
      case 'field':
        return _value(_entry(args['id']! as String), args['key']! as String);
      case 'get':
        final entry = _entry(args['id']! as String);
        return {
          for (final key in ['Title', 'UserName', 'Password', 'URL', 'Notes']) key: _value(entry, key),
          'icon': entry.icon.get()?.index,
          'custom': {
            for (final field in entry.stringEntries)
              if (![
                'Title',
                'UserName',
                'Password',
                'URL',
                'Notes',
              ].any((k) => k.toLowerCase() == field.key.key.toLowerCase()))
                field.key.key: field.value?.getText() ?? '',
          },
        };
      case 'password':
        return _value(_entry(args['id']! as String), 'Password');
      case 'put':
        final icon = _preparedIcon(args);
        if (args['custom'] case final Map<Object?, Object?> fields) {
          final names = <String>{};
          for (final field in fields.entries) {
            final name = (field.key! as String).trim();
            if (name.isEmpty ||
                name.length > 256 ||
                ['title', 'username', 'password', 'url', 'notes'].contains(name.toLowerCase()) ||
                !names.add(name.toLowerCase())) {
              throw const KdbxFailure('name');
            }
          }
        }
        final id = args['id'] as String?;
        final group = _group(args['group'] as String?);
        final entry = id == null ? KdbxEntry.create(db, group) : _entry(id);
        if (id == null) group.addEntry(entry);
        _applyIcon(entry, args, icon);
        for (final key in ['Title', 'UserName', 'Password', 'URL', 'Notes']) {
          final value = args[key]! as String;
          final old = entry.getString(KdbxKey(key));
          entry.setString(
            KdbxKey(key),
            key == 'Password' || old is ProtectedValue ? ProtectedValue.fromString(value) : PlainValue(value),
          );
        }
        if (args['custom'] case final Map<Object?, Object?> fields) {
          final names = fields.keys.map((k) => (k! as String).trim().toLowerCase()).toSet();
          for (final old in entry.stringEntries.toList()) {
            if (!['title', 'username', 'password', 'url', 'notes'].contains(old.key.key.toLowerCase()) &&
                !names.contains(old.key.key.toLowerCase())) {
              entry.removeString(old.key);
            }
          }
          for (final field in fields.entries) {
            entry.setString(KdbxKey((field.key! as String).trim()), ProtectedValue.fromString(field.value! as String));
          }
        }
        if (entry.parent != group) db.move(entry, group);
        _dirty = true;
        return snapshot();
      case 'move':
        final entry = _entry(args['id']! as String);
        final group = _group(args['group']! as String);
        if (entry.parent != group) {
          db.move(entry, group);
          _dirty = true;
        }
        return snapshot();
      case 'group':
        final name = args['name']! as String;
        if (name.trim().isEmpty) throw const KdbxFailure('name');
        final icon = _preparedIcon(args);
        final group = db.createGroup(parent: _group(args['parent'] as String?), name: name.trim());
        _applyIcon(group, args, icon);
        _dirty = true;
        return snapshot();
      case 'delete':
        db.deleteEntry(_entry(args['id']! as String));
        _dirty = true;
        return snapshot();
      case 'renameGroup':
        final group = _editableGroup(args['id']! as String);
        final name = (args['name']! as String).trim();
        if (name.isEmpty) throw const KdbxFailure('name');
        final icon = _preparedIcon(args);
        if (group.name.get() != name) {
          group.name.set(name);
          _dirty = true;
        }
        if (icon != null || args['icon'] != null) {
          _applyIcon(group, args, icon);
          _dirty = true;
        }
        return snapshot();
      case 'deleteGroup':
        final group = _editableGroup(args['id']! as String);
        // KeePass allows the recycle bin below an ordinary folder. Moving
        // that ancestor into its own descendant detaches the entire subtree.
        if (db.recycleBin?.isInGroup(group) == true) throw const KdbxFailure('group');
        db.deleteGroup(group);
        _dirty = true;
        return snapshot();
      case 'purgeGroup':
        final group = _tree(db.body.rootGroup).where((g) => g.uuid.toString() == args['id']).firstOrNull;
        if (db.recycleBin?.parent == null ||
            group == null ||
            group.parent == null ||
            group == db.recycleBin ||
            !_recycledGroup(group)) {
          throw const KdbxFailure('group');
        }
        db.deletePermanently(group);
        _dirty = true;
        return snapshot();
      case 'purgeEntry':
        final entry = _entry(args['id']! as String);
        if (db.recycleBin?.parent == null || entry.parent == null || !_recycledGroup(entry.parent!)) {
          throw const KdbxFailure('group');
        }
        db.deletePermanently(entry);
        _dirty = true;
        return snapshot();
      case 'emptyTrash':
        final bin = db.recycleBin;
        // Bind the destructive request to the actual system bin, not a name.
        // Never create a bin as a side effect of clearing an absent/stale one.
        if (bin == null || bin.parent == null || bin.uuid.toString() != args['id']) {
          throw const KdbxFailure('group');
        }
        final groups = bin.groups.toList();
        final entries = bin.entries.toList();
        for (final group in groups) {
          db.deletePermanently(group);
        }
        for (final entry in entries) {
          db.deletePermanently(entry);
        }
        if (groups.isNotEmpty || entries.isNotEmpty) _dirty = true;
        return snapshot();
      case 'save':
        if (_remote) throw const KdbxFailure('unsupported');
        await _save();
        return snapshot();
      case 'exportFile':
        final target = File(args['path']! as String);
        if (await target.exists()) throw const KdbxFailure('exists');
        final bytes = await db.save();
        await target.create(exclusive: true);
        await target.writeAsBytes(bytes, flush: true);
        return snapshot();
      default:
        throw const KdbxFailure('unsupported');
    }
  }

  Future<void> _checkUnchanged() async {
    final target = File(_path);
    if (_fingerprint == null) {
      if (await target.exists()) throw const KdbxFailure('changed');
    } else {
      if (!await target.exists() || sha256.convert(await _read(_path, 128 * 1024 * 1024)).toString() != _fingerprint) {
        throw const KdbxFailure('changed');
      }
    }
  }

  Future<void> _save() async {
    await _checkUnchanged();
    // Only encrypted bytes reach disk. Backup and temporary files share the
    // source directory and its ACL. Never delete the source before replacement.
    final stamp = DateTime.now().microsecondsSinceEpoch;
    final temporary = File('$_path.cc-$pid-$stamp.tmp');
    try {
      await db.saveTo((bytes) async {
        await temporary.writeAsBytes(bytes, flush: true);
        await _checkUnchanged();
        if (_fingerprint != null) await File(_path).copy('$_path.$stamp.bak');
        await _checkUnchanged();
        await temporary.rename(_path);
        _fingerprint = sha256.convert(bytes).toString();
      });
      _dirty = false;
    } finally {
      if (await temporary.exists()) await temporary.delete();
    }
  }
}
