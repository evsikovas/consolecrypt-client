import 'dart:convert';
import 'dart:ui' as ui;

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

const _family = 'ConsoleCryptSymbols';
const _tint = Color(0xff5386dd);

Future<Uint8List?> chooseKdbxNerdIcon(BuildContext context) async {
  final code = await showDialog<int>(context: context, builder: (_) => const _Picker());
  if (code == null || !context.mounted) return null;
  final recorder = ui.PictureRecorder();
  final painter = TextPainter(
    text: TextSpan(
      text: String.fromCharCode(code),
      style: const TextStyle(fontFamily: _family, fontSize: 32, color: _tint),
    ),
    textDirection: TextDirection.ltr,
  )..layout();
  painter.paint(Canvas(recorder), Offset((48 - painter.width) / 2, (48 - painter.height) / 2));
  final picture = recorder.endRecording();
  painter.dispose();
  try {
    final image = await picture.toImage(48, 48);
    try {
      return (await image.toByteData(format: ui.ImageByteFormat.png))?.buffer.asUint8List();
    } finally {
      image.dispose();
    }
  } finally {
    picture.dispose();
  }
}

class _Picker extends StatefulWidget {
  const _Picker();

  @override
  State<_Picker> createState() => _PickerState();
}

class _PickerState extends State<_Picker> {
  late final _catalog = rootBundle
      .loadString('assets/nerd-fonts/symbols.json')
      .then((text) => (jsonDecode(text) as Map<String, dynamic>).cast<String, int>());
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Dialog(
      child: SizedBox(
        width: 512,
        height: MediaQuery.sizeOf(context).height.clamp(200, 540).toDouble() * .8,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.keepassNerdIconTitle, style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              TextField(
                autofocus: true,
                decoration: InputDecoration(hintText: l.keepassNerdIconSearch, prefixIcon: const Icon(Icons.search)),
                onChanged: (value) => setState(() => _query = value.trim().toLowerCase().replaceAll(' ', '_')),
              ),
              const SizedBox(height: 8),
              Expanded(
                child: FutureBuilder<Map<String, int>>(
                  future: _catalog,
                  builder: (context, snapshot) {
                    if (snapshot.hasError) return Center(child: Text(l.keepassNerdIconLoadFailed));
                    if (!snapshot.hasData) return const Center(child: CircularProgressIndicator());
                    final matches = snapshot.data!.entries.where((entry) => entry.key.contains(_query)).toList();
                    return GridView.builder(
                      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(maxCrossAxisExtent: 52),
                      itemCount: matches.length,
                      itemBuilder: (context, index) {
                        final entry = matches[index];
                        return Tooltip(
                          message: entry.key.replaceAll('_', ' '),
                          child: InkWell(
                            onTap: () => Navigator.pop(context, entry.value),
                            child: Center(
                              child: Text(
                                String.fromCharCode(entry.value),
                                style: const TextStyle(fontFamily: _family, fontSize: 24, color: _tint),
                              ),
                            ),
                          ),
                        );
                      },
                    );
                  },
                ),
              ),
              Align(
                alignment: AlignmentDirectional.centerEnd,
                child: TextButton(onPressed: () => Navigator.pop(context), child: Text(l.commonCancel)),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
