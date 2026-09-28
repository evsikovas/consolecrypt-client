import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:material_ui/material_ui.dart';

Future<int?> pickRgbColor(BuildContext context, {required String title, required int color}) => showAppDialog<int>(
  context,
  builder: (_) => _ColorPicker(title: title, color: color),
);

class _ColorPicker extends StatefulWidget {
  const _ColorPicker({required this.title, required this.color});
  final String title;
  final int color;
  @override
  State<_ColorPicker> createState() => _ColorPickerState();
}

class _ColorPickerState extends State<_ColorPicker> {
  late HSVColor _hsv = HSVColor.fromColor(Color(0xFF000000 | widget.color));
  late final _hex = TextEditingController(text: TerminalColors.hex(widget.color));
  bool _invalid = false;
  int get rgb => _hsv.toColor().toARGB32() & 0xFFFFFF;
  void update(HSVColor value) => setState(() {
    _hsv = value;
    _hex.text = TerminalColors.hex(rgb);
    _invalid = false;
  });
  void editHex(String text) => setState(() {
    _invalid = !RegExp(r'^#?[0-9a-fA-F]{6}$').hasMatch(text.trim());
    if (!_invalid) {
      _hsv = HSVColor.fromColor(Color(0xFF000000 | int.parse(text.trim().replaceFirst('#', ''), radix: 16)));
    }
  });
  @override
  void dispose() {
    _hex.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return GlassDialog(
      title: widget.title,
      width: 480,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            LayoutBuilder(
              builder: (context, constraints) {
                const height = 180.0;
                void point(Offset offset) => update(
                  _hsv
                      .withSaturation((offset.dx / constraints.maxWidth).clamp(0, 1))
                      .withValue((1 - offset.dy / height).clamp(0, 1)),
                );
                return Semantics(
                  label: l.colorSpectrum,
                  child: GestureDetector(
                    key: const ValueKey('color-spectrum'),
                    onPanDown: (d) => point(d.localPosition),
                    onPanUpdate: (d) => point(d.localPosition),
                    child: SizedBox(
                      height: height,
                      width: double.infinity,
                      child: CustomPaint(painter: _SpectrumPainter(_hsv)),
                    ),
                  ),
                );
              },
            ),
            const SizedBox(height: 12),
            Text(l.colorHue, style: tokens.typography.caption),
            DecoratedBox(
              decoration: const BoxDecoration(
                gradient: LinearGradient(
                  colors: [
                    Color(0xFFFF0000),
                    Color(0xFFFFFF00),
                    Color(0xFF00FF00),
                    Color(0xFF00FFFF),
                    Color(0xFF0000FF),
                    Color(0xFFFF00FF),
                    Color(0xFFFF0000),
                  ],
                ),
                borderRadius: BorderRadius.all(Radius.circular(12)),
              ),
              child: Slider(
                key: const ValueKey('color-hue'),
                value: _hsv.hue,
                max: 360,
                activeColor: Colors.transparent,
                inactiveColor: Colors.transparent,
                thumbColor: Colors.white,
                label: '${_hsv.hue.round()}°',
                semanticFormatterCallback: (v) => '${l.colorHue}: ${v.round()}°',
                onChanged: (v) => update(_hsv.withHue(v)),
              ),
            ),
            // Sliders provide keyboard and accessibility alternatives to the spectrum.
            Text(l.colorSaturation, style: tokens.typography.caption),
            Slider(
              key: const ValueKey('color-saturation'),
              value: _hsv.saturation,
              semanticFormatterCallback: (v) => '${l.colorSaturation}: ${(v * 100).round()}%',
              onChanged: (v) => update(_hsv.withSaturation(v)),
            ),
            Text(l.colorBrightness, style: tokens.typography.caption),
            Slider(
              key: const ValueKey('color-brightness'),
              value: _hsv.value,
              semanticFormatterCallback: (v) => '${l.colorBrightness}: ${(v * 100).round()}%',
              onChanged: (v) => update(_hsv.withValue(v)),
            ),
            Row(
              children: [
                Container(
                  width: 48,
                  height: 40,
                  decoration: BoxDecoration(
                    color: _hsv.toColor(),
                    border: Border.all(color: tokens.secondaryLabel),
                    borderRadius: BorderRadius.circular(8),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: TextField(
                    key: const ValueKey('color-picker-hex'),
                    controller: _hex,
                    onChanged: editHex,
                    maxLength: 7,
                    style: tokens.typography.mono,
                    decoration: InputDecoration(
                      labelText: l.settingsColorHex,
                      counterText: '',
                      errorText: _invalid ? l.settingsColorInvalid : null,
                    ),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('color-picker-cancel'),
          label: l.commonCancel,
          onPressed: () => closeDialog<int>(context),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('color-picker-apply'),
        label: l.settingsColorApply,
        onPressed: _invalid ? null : () => closeDialog(context, rgb),
      ),
    );
  }
}

class _SpectrumPainter extends CustomPainter {
  const _SpectrumPainter(this.hsv);
  final HSVColor hsv;
  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    canvas.drawRect(
      rect,
      Paint()
        ..shader = LinearGradient(colors: [Colors.white, HSVColor.fromAHSV(1, hsv.hue, 1, 1).toColor()])
            .createShader(rect),
    );
    canvas.drawRect(
      rect,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Colors.transparent, Colors.black],
        ).createShader(rect),
    );
    final p = Offset(hsv.saturation * size.width, (1 - hsv.value) * size.height);
    canvas.drawCircle(
      p,
      7,
      Paint()
        ..color = Colors.black
        ..style = PaintingStyle.stroke
        ..strokeWidth = 4,
    );
    canvas.drawCircle(
      p,
      7,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
  }

  @override
  bool shouldRepaint(_SpectrumPainter oldDelegate) => oldDelegate.hsv != hsv;
}
