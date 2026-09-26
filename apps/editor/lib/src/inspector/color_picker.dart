// The colour picker, as Unity's (docs/ui-refresh): the new colour beside
// the old one (a click on the old puts it back), a hue ring around the
// saturation/value square, one slider per channel whose track shows where
// it leads — RGB 0–255, RGB 0–1 or HSV — with its number beside it, the
// hexadecimal, Unity's default palette and the user's own swatches (kept
// in the preferences). Every change is shown in the world at once; OK
// commits it as one step, Cancel puts it back.
part of 'field_widgets.dart';

/// The user's saved colours (Unity's Swatches) as `#RRGGBB[AA]`. The app
/// points these at the preferences; without it they live for the session.
abstract final class ColorSwatches {
  static List<String> _session = [];
  static List<String> Function() read = () => _session;
  static void Function(List<String> hexes) write = (hexes) => _session = hexes;
}

/// How the channel sliders read.
enum ColorPickerMode {
  rgb255('RGB 0–255'),
  rgb1('RGB 0–1'),
  hsv('HSV');

  const ColorPickerMode(this.label);
  final String label;
}

/// HSV picker over sRGB; the result is linear RGB(A).
class ColorPickerDialog extends StatefulWidget {
  const ColorPickerDialog({super.key, required this.initial, this.alpha = false, this.onChanged});

  final List<double> initial;
  final bool alpha;

  /// Every change while picking, as linear RGB(A).
  final void Function(List<double> rgb)? onChanged;

  /// The last mode and folds, for the next picker (Unity keeps them).
  static ColorPickerMode mode = ColorPickerMode.rgb255;
  static bool defaultsOpen = true;
  static bool swatchesOpen = true;

  @override
  State<ColorPickerDialog> createState() => _ColorPickerDialogState();
}

class _ColorPickerDialogState extends State<ColorPickerDialog> {
  double _h = 0;
  late double _s, _v, _a;

  @override
  void initState() {
    super.initState();
    _setLinear(widget.initial);
  }

  /// Hue is kept when a grey has none, so the ring does not jump to red.
  void _setSrgb(double r, double g, double b) {
    final (h, s, v) = rgbToHsv(r, g, b);
    if (s > 0 && v > 0) _h = h;
    _s = s;
    _v = v;
  }

  void _setLinear(List<double> rgb) {
    _setSrgb(linearToSrgb(rgb[0]), linearToSrgb(rgb[1]), linearToSrgb(rgb[2]));
    _a = rgb.length > 3 ? rgb[3].clamp(0.0, 1.0) : 1;
  }

  (double, double, double) get _srgb => hsvToRgb(_h, _s, _v);

  List<double> get _linear {
    final (r, g, b) = _srgb;
    return [srgbToLinear(r), srgbToLinear(g), srgbToLinear(b), if (widget.alpha) _a];
  }

  Color get _color {
    final (r, g, b) = _srgb;
    return Color.from(alpha: widget.alpha ? _a : 1, red: r, green: g, blue: b);
  }

  Color get _old {
    final i = widget.initial;
    return Color.from(
      alpha: widget.alpha && i.length > 3 ? i[3].clamp(0.0, 1.0) : 1,
      red: linearToSrgb(i[0]),
      green: linearToSrgb(i[1]),
      blue: linearToSrgb(i[2]),
    );
  }

  void _change(VoidCallback fn) {
    setState(fn);
    widget.onChanged?.call(_linear);
  }

  void _pickHex(String text) {
    final rgb = hexToLinearRgb(text);
    if (rgb == null) return;
    _change(() {
      final a = _a;
      _setLinear(rgb);
      if (rgb.length < 4) _a = a;
    });
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final small = theme.textTheme.bodySmall?.copyWith(color: t.textSecondary);
    return Dialog(
      insetPadding: const EdgeInsets.all(24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 300),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 12, 14, 8),
              child: Row(
                children: [
                  Text('Colour', style: theme.textTheme.titleSmall),
                  const Spacer(),
                  _preview(context),
                ],
              ),
            ),
            Flexible(
              child: SingleChildScrollView(
                padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Center(child: _wheel(context)),
                    const SizedBox(height: 10),
                    Align(alignment: Alignment.centerRight, child: _modeMenu(context)),
                    const SizedBox(height: 6),
                    ..._channels(context),
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        Expanded(child: Text('Hexadecimal', style: small)),
                        SizedBox(
                          width: 120,
                          child: _CommitText(
                            key: const ValueKey('picker.hex'),
                            text: linearRgbToHex(_linear),
                            monospace: true,
                            onCommit: _pickHex,
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 10),
                    _fold(
                      context,
                      key: 'picker.defaults',
                      title: 'Defaults',
                      open: ColorPickerDialog.defaultsOpen,
                      onToggle: () => setState(() => ColorPickerDialog.defaultsOpen = !ColorPickerDialog.defaultsOpen),
                    ),
                    if (ColorPickerDialog.defaultsOpen)
                      _grid(context, [
                        for (final (i, c) in _defaults.indexed) _chip(context, 'picker.default.$i', c, () => _pickColor(c)),
                      ]),
                    const SizedBox(height: 6),
                    _fold(
                      context,
                      key: 'picker.swatches',
                      title: 'Swatches',
                      open: ColorPickerDialog.swatchesOpen,
                      onToggle: () => setState(() => ColorPickerDialog.swatchesOpen = !ColorPickerDialog.swatchesOpen),
                    ),
                    if (ColorPickerDialog.swatchesOpen) _swatches(context),
                  ],
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 4, 14, 12),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
                  const SizedBox(width: 6),
                  FilledButton(onPressed: () => Navigator.of(context).pop(_linear), child: const Text('OK')),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// New | old, over a checkerboard when there is alpha; the old half puts
  /// the starting colour back.
  Widget _preview(BuildContext context) {
    final t = context.tokens;
    Widget half(Color c, {Key? key, String? tip, VoidCallback? onTap}) {
      final box = CustomPaint(
        painter: _CheckerPainter(on: widget.alpha),
        child: ColoredBox(color: c, child: const SizedBox(width: 44, height: 22)),
      );
      if (onTap == null) return KeyedSubtree(key: key, child: box);
      return Tooltip(
        message: tip!,
        waitDuration: const Duration(milliseconds: 500),
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          child: GestureDetector(key: key, onTap: onTap, child: box),
        ),
      );
    }

    return Tooltip(
      message: 'Linear ${_linear.map((c) => c.toStringAsFixed(3)).join(', ')}',
      waitDuration: const Duration(milliseconds: 600),
      child: Container(
        decoration: BoxDecoration(border: Border.all(color: t.lineStrong)),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            half(_color, key: const ValueKey('picker.new')),
            Container(width: 1, height: 22, color: t.lineStrong),
            half(
              _old,
              key: const ValueKey('picker.old'),
              tip: 'The colour before: click to go back to it',
              onTap: () => _change(() => _setLinear(widget.initial)),
            ),
          ],
        ),
      ),
    );
  }

  static const _wheelSize = 224.0;
  static const _ring = 18.0;

  /// Unity's wheel: the hue on a ring, saturation and value in the square
  /// inside it.
  Widget _wheel(BuildContext context) {
    const size = _wheelSize;
    final inner = (size / 2 - _ring - 8) * math.sqrt2;
    void hueAt(Offset p) {
      final c = p - const Offset(size / 2, size / 2);
      if (c.distance < 1) return;
      // Counter-clockwise from red at the right, as Unity's.
      final deg = -math.atan2(c.dy, c.dx) * 180 / math.pi;
      _change(() => _h = (deg + 360) % 360);
    }

    return SizedBox.square(
      dimension: size,
      child: Stack(
        children: [
          Positioned.fill(
            child: GestureDetector(
              key: const ValueKey('picker.hue'),
              onPanDown: (d) => hueAt(d.localPosition),
              onPanUpdate: (d) => hueAt(d.localPosition),
              child: CustomPaint(painter: _RingPainter(_h, ring: _ring)),
            ),
          ),
          Center(
            child: SizedBox.square(
              dimension: inner,
              child: GestureDetector(
                key: const ValueKey('picker.sv'),
                onPanDown: (d) => _svAt(d.localPosition, Size.square(inner)),
                onPanUpdate: (d) => _svAt(d.localPosition, Size.square(inner)),
                child: CustomPaint(painter: _SvPainter(_h, _s, _v), size: Size.square(inner)),
              ),
            ),
          ),
        ],
      ),
    );
  }

  void _svAt(Offset p, Size size) => _change(() {
    _s = (p.dx / size.width).clamp(0.0, 1.0);
    _v = 1 - (p.dy / size.height).clamp(0.0, 1.0);
  });

  Widget _modeMenu(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    return PopupMenuButton<ColorPickerMode>(
      key: const ValueKey('picker.mode'),
      popUpAnimationStyle: AnimationStyle.noAnimation,
      tooltip: 'How the sliders read',
      onSelected: (m) => setState(() => ColorPickerDialog.mode = m),
      itemBuilder: (_) => [
        for (final m in ColorPickerMode.values)
          PopupMenuItem(
            key: ValueKey('picker.mode.${m.name}'),
            value: m,
            height: 26,
            child: Row(
              children: [
                SizedBox(width: 18, child: m == ColorPickerDialog.mode ? const Icon(FIcons.check, size: 14) : null),
                Text(m.label, style: small?.copyWith(color: t.text)),
              ],
            ),
          ),
      ],
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
        decoration: BoxDecoration(color: t.field, borderRadius: t.cornerControl),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(ColorPickerDialog.mode.label, style: small?.copyWith(color: t.text)),
            const SizedBox(width: 4),
            Icon(FIcons.chevronDown, size: 12, color: t.textSecondary),
          ],
        ),
      ),
    );
  }

  /// One row per channel: its letter, a slider whose track shows the
  /// colour along it, its number.
  List<Widget> _channels(BuildContext context) {
    final mode = ColorPickerDialog.mode;
    final (r, g, b) = _srgb;
    Color rgb(double r, double g, double b) => Color.from(alpha: 1, red: r, green: g, blue: b);
    Color hsv(double h, double s, double v) {
      final (r, g, b) = hsvToRgb(h, s, v);
      return rgb(r, g, b);
    }

    // Unity's numbers: 0–255, 0–1, or H in degrees and S, V, A in percent.
    String shown(double unit, {bool hue = false}) => switch (mode) {
      ColorPickerMode.rgb255 => '${(unit * 255).round()}',
      ColorPickerMode.rgb1 => unit.toStringAsFixed(3),
      ColorPickerMode.hsv => hue ? '${(unit * 360).round()}' : '${(unit * 100).round()}',
    };
    double? parse(String text, {bool hue = false}) {
      final v = double.tryParse(text.trim());
      if (v == null) return null;
      final unit = switch (mode) {
        ColorPickerMode.rgb255 => v / 255,
        ColorPickerMode.rgb1 => v,
        ColorPickerMode.hsv => hue ? v / 360 : v / 100,
      };
      return unit.clamp(0.0, 1.0);
    }

    void setRgb(int i, double v) => _change(() {
      final c = [r, g, b]..[i] = v;
      _setSrgb(c[0], c[1], c[2]);
    });

    final rows = <(String, String, double, List<Color>, void Function(double), bool)>[
      if (mode == ColorPickerMode.hsv) ...[
        ('H', 'h', _h / 360, [for (var i = 0; i <= 6; i++) hsv(i * 60.0, 1, 1)], (v) => _change(() => _h = v * 360), true),
        ('S', 's', _s, [hsv(_h, 0, _v), hsv(_h, 1, _v)], (v) => _change(() => _s = v), false),
        ('V', 'v', _v, [hsv(_h, _s, 0), hsv(_h, _s, 1)], (v) => _change(() => _v = v), false),
      ] else ...[
        ('R', 'r', r, [rgb(0, g, b), rgb(1, g, b)], (v) => setRgb(0, v), false),
        ('G', 'g', g, [rgb(r, 0, b), rgb(r, 1, b)], (v) => setRgb(1, v), false),
        ('B', 'b', b, [rgb(r, g, 0), rgb(r, g, 1)], (v) => setRgb(2, v), false),
      ],
      if (widget.alpha) ('A', 'a', _a, [rgb(r, g, b).withValues(alpha: 0), rgb(r, g, b)], (v) => _change(() => _a = v), false),
    ];
    final t = context.tokens;
    final label = Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textSecondary);
    return [
      for (final (letter, id, value, colors, set, hue) in rows)
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: Row(
            children: [
              SizedBox(width: 16, child: Text(letter, style: label)),
              Expanded(
                child: _ChannelSlider(
                  key: ValueKey('picker.channel.$id'),
                  value: value,
                  colors: colors,
                  checker: id == 'a',
                  onChanged: set,
                ),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 56,
                child: _CommitText(
                  key: ValueKey('picker.value.$id'),
                  text: shown(value, hue: hue),
                  onCommit: (text) {
                    final v = parse(text, hue: hue);
                    if (v != null) set(v);
                  },
                ),
              ),
            ],
          ),
        ),
    ];
  }

  Widget _fold(
    BuildContext context, {
    required String key,
    required String title,
    required bool open,
    required VoidCallback onToggle,
    Widget? trailing,
  }) {
    final t = context.tokens;
    return InkWell(
      key: ValueKey(key),
      onTap: onToggle,
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 3),
        child: Row(
          children: [
            Icon(open ? FIcons.expandMore : FIcons.chevronRight, size: 14, color: t.textSecondary),
            const SizedBox(width: 4),
            Text(title, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.text)),
            if (trailing != null) ...[const Spacer(), trailing],
          ],
        ),
      ),
    );
  }

  static const _columns = 12;

  Widget _grid(BuildContext context, List<Widget> chips) => Padding(
    padding: const EdgeInsets.only(top: 2, bottom: 2),
    child: LayoutBuilder(
      builder: (context, c) {
        final cell = ((c.maxWidth - (_columns - 1) * 2) / _columns).floorToDouble();
        return Wrap(
          spacing: 2,
          runSpacing: 2,
          children: [for (final chip in chips) SizedBox.square(dimension: cell, child: chip)],
        );
      },
    ),
  );

  Widget _chip(BuildContext context, String key, Color c, VoidCallback onTap, {VoidCallback? onRemove}) {
    final t = context.tokens;
    final hex = '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0').toUpperCase()}';
    return Tooltip(
      message: onRemove == null ? hex : '$hex  (right-click to remove)',
      waitDuration: const Duration(milliseconds: 500),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        child: GestureDetector(
          key: ValueKey(key),
          onTap: onTap,
          onSecondaryTap: onRemove,
          child: CustomPaint(
            painter: _CheckerPainter(on: c.a < 1),
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: c,
                border: Border.all(color: t.lineStrong, width: .5),
              ),
            ),
          ),
        ),
      ),
    );
  }

  void _pickColor(Color c) => _change(() {
    _setSrgb(c.r, c.g, c.b);
    if (widget.alpha && c.a < 1) _a = c.a;
  });

  Widget _swatches(BuildContext context) {
    final t = context.tokens;
    final saved = ColorSwatches.read();
    Color? parse(String hex) {
      final rgb = hexToLinearRgb(hex);
      if (rgb == null) return null;
      return Color.from(
        alpha: rgb.length > 3 ? rgb[3] : 1,
        red: linearToSrgb(rgb[0]),
        green: linearToSrgb(rgb[1]),
        blue: linearToSrgb(rgb[2]),
      );
    }

    return _grid(context, [
      for (final (i, hex) in saved.indexed)
        if (parse(hex) case final c?)
          _chip(
            context,
            'picker.swatch.$i',
            c,
            () => _pickColor(c),
            onRemove: () => setState(() => ColorSwatches.write([...saved]..removeAt(i))),
          ),
      Tooltip(
        message: 'Add the current colour to the swatches',
        waitDuration: const Duration(milliseconds: 500),
        child: InkWell(
          key: const ValueKey('picker.swatch.add'),
          onTap: () => setState(() => ColorSwatches.write([...saved, linearRgbToHex(_linear)])),
          child: DecoratedBox(
            decoration: BoxDecoration(border: Border.all(color: t.lineStrong)),
            child: Icon(FIcons.add, size: 12, color: t.textSecondary),
          ),
        ),
      ),
    ]);
  }

  /// Unity's Defaults: a row of plain colours and greys, then the hues at
  /// four strengths.
  static final List<Color> _defaults = () {
    Color hex(int v) => Color(0xFF000000 | v);
    Color hsv(double h, double s, double v) {
      final (r, g, b) = hsvToRgb(h, s, v);
      return Color.from(alpha: 1, red: r, green: g, blue: b);
    }

    return [
      for (final v in [0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0x00FFFF, 0xFF00FF]) hex(v),
      for (var i = 0; i < 6; i++) hsv(0, 0, i / 5),
      for (final (s, v) in [(1.0, .45), (1.0, .8), (.55, 1.0), (.25, 1.0)])
        for (var i = 0; i < _columns; i++) hsv(i * 360 / _columns, s, v),
    ];
  }();
}

/// A channel's slider: the track is the colour along it, the thumb a thin
/// bar; a click or a drag anywhere on it sets the value.
class _ChannelSlider extends StatelessWidget {
  const _ChannelSlider({super.key, required this.value, required this.colors, required this.onChanged, this.checker = false});

  final double value;
  final List<Color> colors;
  final bool checker;
  final ValueChanged<double> onChanged;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return LayoutBuilder(
      builder: (context, c) {
        void at(Offset p) => onChanged((p.dx / c.maxWidth).clamp(0.0, 1.0));
        return GestureDetector(
          onPanDown: (d) => at(d.localPosition),
          onPanUpdate: (d) => at(d.localPosition),
          child: SizedBox(
            height: t.fieldHeight - 4,
            child: CustomPaint(
              painter: _TrackPainter(value: value, colors: colors, checker: checker, edge: t.lineStrong),
            ),
          ),
        );
      },
    );
  }
}

class _TrackPainter extends CustomPainter {
  _TrackPainter({required this.value, required this.colors, required this.checker, required this.edge});

  final double value;
  final List<Color> colors;
  final bool checker;
  final Color edge;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    if (checker) _CheckerPainter.paintChecks(canvas, rect);
    canvas.drawRect(rect, Paint()..shader = LinearGradient(colors: colors).createShader(rect));
    canvas.drawRect(
      rect.deflate(.5),
      Paint()
        ..color = edge
        ..style = PaintingStyle.stroke,
    );
    final x = (value.clamp(0.0, 1.0) * size.width).clamp(2.0, size.width - 2);
    final thumb = Rect.fromCenter(center: Offset(x, size.height / 2), width: 4, height: size.height + 2);
    canvas.drawRect(thumb, Paint()..color = Colors.white);
    canvas.drawRect(
      thumb,
      Paint()
        ..color = Colors.black54
        ..style = PaintingStyle.stroke,
    );
  }

  @override
  bool shouldRepaint(_TrackPainter old) =>
      old.value != value || !listEquals(old.colors, colors) || old.checker != checker || old.edge != edge;
}

/// Grey and white squares behind a colour with alpha.
class _CheckerPainter extends CustomPainter {
  const _CheckerPainter({required this.on});
  final bool on;

  static void paintChecks(Canvas canvas, Rect rect) {
    canvas.save();
    canvas.clipRect(rect);
    canvas.drawRect(rect, Paint()..color = const Color(0xFFCCCCCC));
    const cell = 5.0;
    final dark = Paint()..color = const Color(0xFF999999);
    for (var y = rect.top, row = 0; y < rect.bottom; y += cell, row++) {
      for (var x = rect.left + (row.isOdd ? cell : 0); x < rect.right; x += 2 * cell) {
        canvas.drawRect(Rect.fromLTWH(x, y, cell, cell), dark);
      }
    }
    canvas.restore();
  }

  @override
  void paint(Canvas canvas, Size size) {
    if (on) paintChecks(canvas, Offset.zero & size);
  }

  @override
  bool shouldRepaint(_CheckerPainter old) => old.on != on;
}

/// The hue ring: red at the right, the hues counter-clockwise (Unity's),
/// a circle on the current hue.
class _RingPainter extends CustomPainter {
  _RingPainter(this.h, {required this.ring});
  final double h;
  final double ring;

  @override
  void paint(Canvas canvas, Size size) {
    final center = size.center(Offset.zero);
    final radius = size.shortestSide / 2 - ring / 2;
    Color hue(double deg) {
      final (r, g, b) = hsvToRgb(deg, 1, 1);
      return Color.from(alpha: 1, red: r, green: g, blue: b);
    }

    // A sweep runs clockwise on screen: hue 360 − angle.
    final colors = [for (var i = 0; i <= 12; i++) hue((360 - i * 30) % 360)];
    canvas.drawCircle(
      center,
      radius,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = ring
        ..shader = SweepGradient(colors: colors).createShader(Rect.fromCircle(center: center, radius: radius)),
    );
    final a = -h * math.pi / 180;
    final at = center + Offset(math.cos(a), math.sin(a)) * radius;
    canvas.drawCircle(at, ring / 2 + 1, Paint()..color = hue(h));
    canvas.drawCircle(
      at,
      ring / 2 + 1,
      Paint()
        ..color = Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
  }

  @override
  bool shouldRepaint(_RingPainter old) => old.h != h || old.ring != ring;
}

class _SvPainter extends CustomPainter {
  _SvPainter(this.h, this.s, this.v);
  final double h, s, v;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final (r, g, b) = hsvToRgb(h, 1, 1);
    final hue = Color.fromARGB(255, (r * 255).round(), (g * 255).round(), (b * 255).round());
    canvas.drawRect(rect, Paint()..shader = LinearGradient(colors: [Colors.white, hue]).createShader(rect));
    canvas.drawRect(
      rect,
      Paint()
        ..shader = const LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [Colors.transparent, Colors.black],
        ).createShader(rect),
    );
    final c = Offset(s * size.width, (1 - v) * size.height);
    canvas.drawCircle(
      c,
      6,
      Paint()
        ..color = v > .5 ? Colors.black : Colors.white
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.5,
    );
  }

  @override
  bool shouldRepaint(_SvPainter old) => old.h != h || old.s != s || old.v != v;
}
