import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

const appearanceSwatches = [
  Color(0xff4b89ff),
  Color(0xffff8700),
  Color(0xff20b7ae),
  Color(0xff4fb977),
  Color(0xff9b6bee),
  Color(0xffdb70aa),
  Color(0xffef6068),
  Color(0xff8a97a7),
  Color(0xffffffff),
  Color(0xff000000),
];

/// Swatches and the spectrum edit a draft. Only Apply changes the application.
class AppearanceColorRow extends StatefulWidget {
  const AppearanceColorRow({
    super.key,
    required this.label,
    required this.value,
    required this.fallback,
    required this.onApply,
  });
  final String label;
  final Color? value;
  final Color fallback;
  final Future<String?> Function(Color?) onApply;

  @override
  State<AppearanceColorRow> createState() => _AppearanceColorRowState();
}

class _AppearanceColorRowState extends State<AppearanceColorRow> {
  late final hex = TextEditingController(text: colorToHex(widget.value ?? widget.fallback));
  String? error;
  bool saving = false;

  @override
  void didUpdateWidget(AppearanceColorRow oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.value != widget.value || oldWidget.value == null && oldWidget.fallback != widget.fallback) {
      hex.text = colorToHex(widget.value ?? widget.fallback);
      error = null;
    }
  }

  @override
  void dispose() {
    hex.dispose();
    super.dispose();
  }

  void draft(Color color) => setState(() {
    hex.text = colorToHex(color);
    error = null;
  });

  Future<void> apply({bool reset = false}) async {
    final color = reset ? null : parseHexColor(hex.text);
    if (!reset && color == null) {
      setState(() => error = 'Введите 6 HEX-цифр, например #4B89FF');
      return;
    }
    setState(() {
      saving = true;
      error = null;
    });
    final message = await widget.onApply(color);
    if (!mounted) return;
    setState(() {
      saving = false;
      if (reset) hex.text = colorToHex(widget.fallback);
      error = message;
    });
  }

  Future<void> spectrum() async {
    final color = await showWslcDialog<Color>(
      context: context,
      builder: (_) =>
          _SpectrumDialog(initial: parseHexColor(hex.text) ?? widget.value ?? widget.fallback, label: widget.label),
    );
    if (color != null && mounted) draft(color);
  }

  @override
  Widget build(BuildContext context) {
    final palette = DeskPalette.of(context);
    final selected = parseHexColor(hex.text);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(widget.label, style: const TextStyle(fontWeight: FontWeight.w600)),
        const SizedBox(height: 10),
        Wrap(
          spacing: 7,
          runSpacing: 8,
          children: [
            for (final color in appearanceSwatches)
              Tooltip(
                message: colorToHex(color),
                child: Semantics(
                  label: '${widget.label}: ${colorToHex(color)}',
                  button: true,
                  selected: selected == color,
                  child: InkResponse(
                    onTap: saving ? null : () => draft(color),
                    radius: 19,
                    child: Container(
                      width: 28,
                      height: 28,
                      decoration: BoxDecoration(
                        color: color,
                        shape: BoxShape.circle,
                        border: Border.all(
                          color: selected == color ? palette.text : palette.border,
                          width: selected == color ? 2 : 1,
                        ),
                      ),
                      child: selected == color
                          ? Icon(Icons.check_rounded, size: 18, color: contrastingText(color))
                          : null,
                    ),
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 12),
        Wrap(
          spacing: 10,
          runSpacing: 10,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            SizedBox(
              width: 174,
              child: TextField(
                controller: hex,
                enabled: !saving,
                key: ValueKey('${widget.label}-hex'),
                maxLength: 7,
                decoration: InputDecoration(
                  labelText: 'HEX-код цвета',
                  semanticCounterText: widget.label,
                  counterText: '',
                ),
                style: const TextStyle(fontFamily: monoFont, fontSize: 13),
                onChanged: (_) => setState(() => error = null),
                onSubmitted: (_) => apply(),
              ),
            ),
            OutlinedButton.icon(
              onPressed: saving ? null : spectrum,
              icon: const Icon(Icons.palette_outlined, size: 18),
              label: const Text('Спектр цветов'),
            ),
            FilledButton(onPressed: saving ? null : () => apply(), child: const Text('Применить')),
            TextButton(
              onPressed: saving || widget.value == null ? null : () => apply(reset: true),
              child: const Text('По умолчанию'),
            ),
          ],
        ),
        if (error != null)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(error!, style: TextStyle(color: palette.danger)),
          ),
      ],
    );
  }
}

class _SpectrumDialog extends StatefulWidget {
  const _SpectrumDialog({required this.initial, required this.label});
  final Color initial;
  final String label;

  @override
  State<_SpectrumDialog> createState() => _SpectrumDialogState();
}

class _SpectrumDialogState extends State<_SpectrumDialog> {
  late HSVColor color = HSVColor.fromColor(widget.initial);

  void pick(Offset point, Size size) => setState(() {
    color = color
        .withSaturation((point.dx / size.width).clamp(0, 1))
        .withValue((1 - point.dy / size.height).clamp(0, 1));
  });

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: Text('Спектр цветов — ${widget.label.toLowerCase()}'),
    content: SizedBox(
      width: 380,
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            LayoutBuilder(
              builder: (context, box) {
                final size = Size(box.maxWidth, 200);
                return GestureDetector(
                  onPanDown: (e) => pick(e.localPosition, size),
                  onPanUpdate: (e) => pick(e.localPosition, size),
                  child: CustomPaint(size: size, painter: _SpectrumPainter(color)),
                );
              },
            ),
            const SizedBox(height: 16),
            Container(
              height: 32,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: color.toColor(),
                borderRadius: BorderRadius.circular(6),
                border: Border.all(color: DeskPalette.of(context).border),
              ),
              child: Text(
                colorToHex(color.toColor()),
                style: TextStyle(color: contrastingText(color.toColor()), fontFamily: monoFont),
              ),
            ),
            const SizedBox(height: 12),
            const Text('Оттенок'),
            DecoratedBox(
              decoration: const BoxDecoration(
                gradient: LinearGradient(
                  colors: [
                    Colors.red,
                    Colors.yellow,
                    Colors.green,
                    Colors.cyan,
                    Colors.blue,
                    Colors.purple,
                    Colors.red,
                  ],
                ),
                borderRadius: BorderRadius.all(Radius.circular(8)),
              ),
              child: Slider(
                value: color.hue,
                min: 0,
                max: 360,
                label: '${color.hue.round()}°',
                semanticFormatterCallback: (v) => 'Оттенок ${v.round()} градусов',
                activeColor: Colors.transparent,
                inactiveColor: Colors.transparent,
                thumbColor: Colors.white,
                onChanged: (v) => setState(() => color = color.withHue(v)),
              ),
            ),
            const SizedBox(height: 8),
            const Text('Насыщенность'),
            Slider(
              value: color.saturation,
              label: '${(color.saturation * 100).round()}%',
              semanticFormatterCallback: (v) => 'Насыщенность ${(v * 100).round()} процентов',
              onChanged: (v) => setState(() => color = color.withSaturation(v)),
            ),
            const Text('Яркость'),
            Slider(
              value: color.value,
              label: '${(color.value * 100).round()}%',
              semanticFormatterCallback: (v) => 'Яркость ${(v * 100).round()} процентов',
              onChanged: (v) => setState(() => color = color.withValue(v)),
            ),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
      FilledButton(onPressed: () => Navigator.pop(context, color.toColor()), child: const Text('Выбрать')),
    ],
  );
}

class _SpectrumPainter extends CustomPainter {
  const _SpectrumPainter(this.color);
  final HSVColor color;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    canvas.drawRect(rect, Paint()..color = HSVColor.fromAHSV(1, color.hue, 1, 1).toColor());
    canvas.drawRect(
      rect,
      Paint()..shader = const LinearGradient(colors: [Colors.white, Color(0x00ffffff)]).createShader(rect),
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
    final point = Offset(color.saturation * size.width, (1 - color.value) * size.height);
    canvas.drawCircle(
      point,
      7,
      Paint()
        ..color = Colors.black
        ..style = PaintingStyle.stroke
        ..strokeWidth = 3,
    );
    canvas.drawCircle(
      point,
      5,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
  }

  @override
  bool shouldRepaint(_SpectrumPainter oldDelegate) => oldDelegate.color != color;
}
