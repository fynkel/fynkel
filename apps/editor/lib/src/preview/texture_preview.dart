// The texture preview (ADR-0030 §6.1), as Unity's: the image fitted over a
// checkerboard, zoomed with the wheel, panned with a drag, F or a double
// click to fit again; RGB / R / G / B / A (one channel as grey), a mip
// slider (the cook builds the whole chain), and the line
// `512x512  RGBA8 sRGB  1.3 MB  mips 10` — the size on the GPU, as Unity
// gives it. No GPU work: Flutter decodes the file. An HDR image is not
// something Flutter reads; it says so until the cooked path draws it.
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../project_browser/project_browser_model.dart' show formatBytes;
import '../theme/tokens.dart';
import 'preview_provider.dart';

const _channels = ['rgb', 'r', 'g', 'b', 'a'];

/// The number of mip levels the cook builds for a [size] image.
int mipCount(Size size) => (math.log(math.max(size.width, size.height)) / math.ln2).floor() + 1;

/// The bytes the whole mip chain takes on the GPU at [bytesPerPixel].
int gpuBytes(Size size, {int bytesPerPixel = 4}) {
  var w = size.width.toInt(), h = size.height.toInt(), total = 0;
  while (true) {
    total += w * h * bytesPerPixel;
    if (w == 1 && h == 1) return total;
    w = math.max(1, w >> 1);
    h = math.max(1, h >> 1);
  }
}

bool _isHdr(String file) => file.toLowerCase().endsWith('.hdr');

final texturePreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'texture',
  build: (context, preview) {
    if (_isHdr(preview.file)) {
      return const _Note('HDR images have no preview yet: Flutter cannot read them.');
    }
    return preview.image(
      preview.file,
      channel: preview.value('channel', 'rgb'),
      mip: preview.value('mip', 0),
      onSize: (size) => preview.set('size', size),
    );
  },
  toolbar: (context, preview) {
    if (_isHdr(preview.file)) return const SizedBox.shrink();
    final channel = preview.value('channel', 'rgb');
    final size = preview.value<Size?>('size', null);
    final mips = size == null ? 1 : mipCount(size);
    final mip = preview.value('mip', 0).clamp(0, mips - 1);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (mips > 1) ...[
          Tooltip(
            message: 'Mip level $mip of ${mips - 1}',
            waitDuration: const Duration(milliseconds: 500),
            child: SizedBox(
              width: 70,
              child: SliderTheme(
                data: SliderTheme.of(context).copyWith(
                  trackHeight: 2,
                  thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5),
                  overlayShape: SliderComponentShape.noOverlay,
                ),
                child: Slider(
                  key: const Key('preview.mip'),
                  value: mip.toDouble(),
                  max: (mips - 1).toDouble(),
                  divisions: mips - 1,
                  onChanged: (v) => preview.set('mip', v.round()),
                ),
              ),
            ),
          ),
          const SizedBox(width: 6),
        ],
        for (final c in _channels)
          PreviewToggle(
            key: Key('preview.channel.$c'),
            label: c.toUpperCase(),
            on: c == channel,
            onTap: () => preview.set('channel', c),
          ),
      ],
    );
  },
  info: (preview) {
    if (_isHdr(preview.file)) return 'RGB32F  linear  (HDR)';
    final size = preview.value<Size?>('size', null);
    if (size == null) return null;
    final linear = preview.importSettings['colorSpace'] == 'linear';
    final mips = mipCount(size);
    final mip = preview.value('mip', 0).clamp(0, mips - 1);
    int at(double v) => math.max(1, v.toInt() >> mip);
    return [
      '${size.width.toInt()}x${size.height.toInt()}',
      if (mip > 0) '(mip $mip: ${at(size.width)}x${at(size.height)})',
      'RGBA8',
      linear ? 'Linear' : 'sRGB',
      formatBytes(gpuBytes(size)),
      'mips $mips',
    ].join('  ');
  },
);

/// A small toggle of a preview's toolbar (a channel, a shape).
class PreviewToggle extends StatelessWidget {
  const PreviewToggle({super.key, required this.label, required this.on, required this.onTap});

  final String label;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return InkWell(
      onTap: onTap,
      borderRadius: t.cornerControl,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 2),
        decoration: BoxDecoration(color: on ? t.selectionQuiet : null, borderRadius: t.cornerControl),
        child: Text(
          label,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: on ? t.text : t.textSecondary),
        ),
      ),
    );
  }
}

class _Note extends StatelessWidget {
  const _Note(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Center(
    child: Padding(
      padding: const EdgeInsets.all(12),
      child: Text(
        text,
        key: const Key('preview.note'),
        textAlign: TextAlign.center,
        style: Theme.of(context).textTheme.bodySmall?.copyWith(color: context.tokens.textDisabled),
      ),
    ),
  );
}

/// A colour matrix that shows one channel as grey (opaque); null for RGB.
List<double>? channelMatrix(String channel) {
  final i = switch (channel) {
    'r' => 0,
    'g' => 1,
    'b' => 2,
    'a' => 3,
    _ => -1,
  };
  if (i < 0) return null;
  final row = List<double>.filled(5, 0)..[i] = 1;
  return [...row, ...row, ...row, 0, 0, 0, 0, 255];
}

/// An image file, fitted, zoomable and pannable, over a checkerboard: the
/// editor's building block for image previews ([PreviewContext.image]).
class PreviewImageView extends StatefulWidget {
  const PreviewImageView({super.key, required this.file, this.channel = 'rgb', this.mip = 0, this.onSize});

  final String file;
  final String channel;
  final int mip;
  final void Function(Size size)? onSize;

  @override
  State<PreviewImageView> createState() => _PreviewImageViewState();
}

class _PreviewImageViewState extends State<PreviewImageView> {
  final _view = TransformationController();
  final _focus = FocusNode(debugLabel: 'preview.image');
  Size? _size;
  ui.Image? _image;
  int? _imageMip;
  String? _error;
  var _loading = 0;

  @override
  void initState() {
    super.initState();
    _view.addListener(_onView);
    _load();
  }

  @override
  void didUpdateWidget(covariant PreviewImageView old) {
    super.didUpdateWidget(old);
    if (old.file != widget.file) {
      _size = null;
      _load();
    } else if (old.mip != widget.mip) {
      _load();
    }
  }

  @override
  void dispose() {
    _view.dispose();
    _focus.dispose();
    _image?.dispose();
    super.dispose();
  }

  void _onView() => setState(() {});

  Future<void> _load() async {
    final ticket = ++_loading;
    try {
      final bytes = await File(widget.file).readAsBytes();
      var size = _size;
      if (size == null) {
        final buffer = await ui.ImmutableBuffer.fromUint8List(bytes);
        final descriptor = await ui.ImageDescriptor.encoded(buffer);
        size = Size(descriptor.width.toDouble(), descriptor.height.toDouble());
        descriptor.dispose();
        buffer.dispose();
      }
      final mip = widget.mip.clamp(0, mipCount(size) - 1);
      final codec = await ui.instantiateImageCodec(
        bytes,
        targetWidth: mip == 0 ? null : math.max(1, size.width.toInt() >> mip),
        targetHeight: mip == 0 ? null : math.max(1, size.height.toInt() >> mip),
      );
      final frame = await codec.getNextFrame();
      codec.dispose();
      if (!mounted || ticket != _loading) {
        frame.image.dispose();
        return;
      }
      final first = _size == null;
      setState(() {
        _image?.dispose();
        _image = frame.image;
        _imageMip = mip;
        _size = size;
        _error = null;
      });
      if (first) widget.onSize?.call(size);
    } catch (e) {
      if (!mounted || ticket != _loading) return;
      setState(() => _error = '$e');
    }
  }

  void _fit() => _view.value = Matrix4.identity();

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    if (_error != null) {
      return _Note('The image could not be read: $_error');
    }
    final image = _image;
    final size = _size;
    if (image == null || size == null) return const SizedBox.expand();
    return Focus(
      focusNode: _focus,
      onKeyEvent: (_, e) {
        if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.keyF) {
          _fit();
          return KeyEventResult.handled;
        }
        return KeyEventResult.ignored;
      },
      child: Listener(
        onPointerDown: (_) => _focus.requestFocus(),
        child: GestureDetector(
          onDoubleTap: _fit,
          child: LayoutBuilder(
            builder: (context, box) {
              const pad = 8.0;
              final fit = math.min((box.maxWidth - pad * 2) / size.width, (box.maxHeight - pad * 2) / size.height);
              final scale = fit * _view.value.getMaxScaleOnAxis();
              // Magnified: square pixels, as a texture's are.
              final quality = scale >= 2 || (_imageMip ?? 0) > 0 ? FilterQuality.none : FilterQuality.medium;
              final matrix = channelMatrix(widget.channel);
              Widget picture = RawImage(
                key: const Key('preview.image'),
                image: image,
                fit: BoxFit.fill,
                filterQuality: quality,
              );
              if (matrix != null) picture = ColorFiltered(colorFilter: ColorFilter.matrix(matrix), child: picture);
              return InteractiveViewer(
                transformationController: _view,
                minScale: 0.25,
                maxScale: 64,
                boundaryMargin: const EdgeInsets.all(double.infinity),
                child: Center(
                  child: SizedBox(
                    width: math.max(1, size.width * fit),
                    height: math.max(1, size.height * fit),
                    child: CustomPaint(painter: matrix == null ? _Checker(t.field, t.header) : null, child: picture),
                  ),
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

class _Checker extends CustomPainter {
  _Checker(this.a, this.b);

  final Color a;
  final Color b;

  @override
  void paint(Canvas canvas, Size size) {
    const cell = 8.0;
    canvas.drawRect(Offset.zero & size, Paint()..color = a);
    final paint = Paint()..color = b;
    for (var y = 0.0; y < size.height; y += cell) {
      for (var x = ((y / cell).floor().isOdd ? cell : 0.0); x < size.width; x += cell * 2) {
        canvas.drawRect(Rect.fromLTWH(x, y, math.min(cell, size.width - x), math.min(cell, size.height - y)), paint);
      }
    }
  }

  @override
  bool shouldRepaint(_Checker old) => old.a != a || old.b != b;
}
