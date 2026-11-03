import 'dart:convert';
import 'dart:typed_data';

import 'package:collection/collection.dart' show IterableExtension;
import 'package:convert/convert.dart' as convert;
import 'package:crypto/crypto.dart' as crypto;
import 'package:kdbx/src/credentials/credentials.dart';
import 'package:kdbx/src/crypto/protected_value.dart';
import 'package:kdbx/src/kdbx_exceptions.dart';
import 'package:kdbx/src/utils/byte_utils.dart';
import 'package:meta/meta.dart';
import 'package:xml/xml.dart' as xml;


const _nodeVersion = 'Version';
const _nodeKey = 'Key';
const _nodeData = 'Data';
const _nodeMeta = 'Meta';
const _nodeKeyFile = 'KeyFile';
const _nodeHash = 'Hash';

class KeyFileCredentials implements CredentialsPart {
  factory KeyFileCredentials(Uint8List keyFileContents) {
    if (keyFileContents.length == 32) {
      return KeyFileCredentials._(ProtectedValue.fromBinary(keyFileContents));
    }
    String? text;
    try { text = utf8.decode(keyFileContents); } on FormatException { /* binary key file */ }
    if (text != null && _hexValuePattern.hasMatch(text)) {
      return KeyFileCredentials._(ProtectedValue.fromBinary(Uint8List.fromList(convert.hex.decode(text))));
    }
    if (text != null && text.trimLeft().startsWith('<')) {
      xml.XmlDocument? document;
      try { document = xml.XmlDocument.parse(text); } catch (_) { /* arbitrary file */ }
      if (document?.rootElement.name.local == _nodeKeyFile) {
        try {
          final root = document!.rootElement;
          final version = root.findElements(_nodeMeta).single.findElements(_nodeVersion).single.innerText;
          final data = root.findElements(_nodeKey).single.findElements(_nodeData).single;
          final encoded = data.innerText.replaceAll(RegExp(r'\s'), '');
          final Uint8List bytes;
          if (version.startsWith('2.')) {
            bytes = Uint8List.fromList(convert.hex.decode(encoded));
            final hash = data.getAttribute(_nodeHash);
            if (hash == null || !ByteUtils.eq(convert.hex.decode(hash), crypto.sha256.convert(bytes).bytes.sublist(0, 4))) {
              throw const FormatException('Invalid keyfile hash');
            }
          } else if (version.startsWith('1.')) {
            bytes = base64.decode(encoded);
          } else { throw const FormatException('Unsupported keyfile version'); }
          if (bytes.length != 32) throw const FormatException('Invalid key length');
          return KeyFileCredentials._(ProtectedValue.fromBinary(bytes));
        } catch (_) { throw KdbxInvalidKeyException(); }
      }
    }
    return KeyFileCredentials._(ProtectedValue.fromBinary(Uint8List.fromList(crypto.sha256.convert(keyFileContents).bytes)));
  }

  /// Creates a new random (32 bytes) keyfile value.
  factory KeyFileCredentials.random() => KeyFileCredentials._(
    ProtectedValue.fromBinary(ByteUtils.randomBytes(32)),
  );

  factory KeyFileCredentials.fromBytes(Uint8List bytes) =>
      KeyFileCredentials._(ProtectedValue.fromBinary(bytes));

  KeyFileCredentials._(this._keyFileValue);

  static final RegExp _hexValuePattern = RegExp(
    r'^[a-f\d]{64}',
    caseSensitive: false,
  );

  final ProtectedValue _keyFileValue;

  @override
  Uint8List getBinary() {
    return _keyFileValue.binaryValue;
    //    return crypto.sha256.convert(_keyFileValue.binaryValue).bytes as Uint8List;
  }

  /// Generates a `.keyx` file as described for Keepass keyfile:
  /// https://keepass.info/help/base/keys.html#keyfiles
  Uint8List toXmlV2() {
    return utf8.encode(toXmlV2String());
  }

  /// Generates a `.keyx` file as described for Keepass keyfile:
  /// https://keepass.info/help/base/keys.html#keyfiles
  @visibleForTesting
  String toXmlV2String() {
    final hash =
        (crypto.sha256.convert(_keyFileValue.binaryValue).bytes as Uint8List)
            .sublist(0, 4);
    final hashHexString = hexFormatLikeKeepass(convert.hex.encode(hash));
    final keyHexString = hexFormatLikeKeepass(
      convert.hex.encode(_keyFileValue.binaryValue),
    );

    final builder = xml.XmlBuilder()
      ..processing('xml', 'version="1.0" encoding="utf-8"');
    builder.element(
      _nodeKeyFile,
      nest: () {
        builder.element(
          _nodeMeta,
          nest: () {
            builder.element(
              _nodeVersion,
              nest: () {
                builder.text('2.0');
              },
            );
          },
        );
        builder.element(
          _nodeKey,
          nest: () {
            builder.element(
              _nodeData,
              nest: () {
                builder.attribute(_nodeHash, hashHexString);
                builder.text(keyHexString);
              },
            );
          },
        );
      },
    );
    return builder.buildDocument().toXmlString(pretty: true);
  }

  /// keypass has all-uppercase letters in pairs of 4 bytes (8 characters).
  @visibleForTesting
  static String hexFormatLikeKeepass(final String hexString) {
    final hex = hexString.toUpperCase();
    const groups = 8;
    final remaining = hex.length % groups;
    return [
      for (var i = 0; i < hex.length ~/ groups; i++)
        hex.substring(i * groups, i * groups + groups),
      if (remaining != 0) hex.substring(hex.length - remaining),
    ].join(' ');
    // range(0, hexString.length / 8).map((i) => hexString.substring(i*_groups, i*_groups + _groups));
    // hexString.toUpperCase().chara
  }
}

class KeyFileComposite implements Credentials {
  KeyFileComposite({required this.password, required this.keyFile});

  PasswordCredentials? password;
  KeyFileCredentials? keyFile;

  @override
  Uint8List getHash() {
    final buffer = [...?password?.getBinary(), ...?keyFile?.getBinary()];
    return crypto.sha256.convert(buffer).bytes as Uint8List;

    //    final output = convert.AccumulatorSink<crypto.Digest>();
    //    final input = crypto.sha256.startChunkedConversion(output);
    ////    input.add(password.getHash());
    //    input.add(buffer);
    //    input.close();
    //    return output.events.single.bytes as Uint8List;
  }
}
