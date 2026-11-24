import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/src/rust/api/terminal_codec.dart' as native;

enum TerminalEncoding {
  utf8('UTF-8'),
  gb2312('GB2312'),
  gbk('GBK'),
  gb18030('GB18030'),
  big5('Big5'),
  shiftJis('Shift_JIS'),
  eucJp('EUC-JP'),
  eucKr('EUC-KR'),
  windows1252('Windows-1252'),
  windows1250('Windows-1250'),
  latin1('ISO-8859-1'),
  latin2('ISO-8859-2'),
  latin9('ISO-8859-15'),
  windows1251('Windows-1251'),
  koi8r('KOI8-R');

  const TerminalEncoding(this.label);
  final String label;

  Uint8List? encode(String text) {
    if (this == TerminalEncoding.utf8) return const Utf8Encoder().convert(text);
    if (this == TerminalEncoding.latin1) {
      if (text.codeUnits.any((unit) => unit > 255)) return null;
      return Uint8List.fromList(text.codeUnits);
    }
    return native.encodeTerminalInput(label: label, text: text);
  }
}

final class TerminalByteDecoder {
  TerminalByteDecoder(this.encoding, this.onText) {
    if (encoding == TerminalEncoding.utf8) {
      _utf8 = const Utf8Decoder(allowMalformed: true)
          .startChunkedConversion(StringConversionSink.from(_TextSink(onText)));
    } else if (encoding != TerminalEncoding.latin1) {
      _legacy = native.TerminalDecoder(label: encoding.label);
    }
  }
  final TerminalEncoding encoding;
  final void Function(String) onText;
  ByteConversionSink? _utf8;
  native.TerminalDecoder? _legacy;

  void add(Uint8List bytes) {
    if (_utf8 case final decoder?) {
      decoder.add(bytes);
    } else if (_legacy case final decoder?) {
      onText(decoder.decode(bytes: bytes, last: false));
    } else {
      onText(latin1.decode(bytes));
    }
  }

  void finish() {
    _utf8?.close();
    if (_legacy case final decoder?) onText(decoder.decode(bytes: const [], last: true));
    dispose();
  }

  void dispose() {
    _legacy?.dispose();
    _legacy = null;
    _utf8 = null;
  }
}

final class _TextSink implements Sink<String> {
  const _TextSink(this.onText);
  final void Function(String) onText;
  @override
  void add(String value) => onText(value);
  @override
  void close() {}
}
