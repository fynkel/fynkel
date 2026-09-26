// The Inspector's preview area (ADR-0030), as Unity's: under an asset's
// page, a header with the asset's name and the preview's toolbar, then the
// picture and its info line. Clicking the name folds it; dragging the
// line above it resizes it; both are kept per preview in the preferences.
// What it draws comes from the first provider that takes the asset (a
// package's before the editor's own); a builder that throws shows its
// error in the picture and once in the Console.
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../extensions/editor_extensions.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'preview_provider.dart';
import 'stage_previews.dart';
import 'stage_view.dart';
import 'texture_preview.dart';

/// The area's height and fold per preview, read from and written to the
/// preferences (bound at start; in memory otherwise).
abstract final class PreviewLayout {
  static Map<String, Object?>? Function(String preview) read = (p) => _memory[p];
  static void Function(String preview, Map<String, Object?> layout) write = (p, l) => _memory[p] = l;
  static final _memory = <String, Map<String, Object?>>{};

  static const defaultHeight = 240.0;
  static const minHeight = 80.0;

  static double heightOf(String preview) => (read(preview)?['height'] as num?)?.toDouble() ?? defaultHeight;
  static bool isOpen(String preview) => read(preview)?['open'] as bool? ?? true;
  static void set(String preview, {double? height, bool? open}) =>
      write(preview, {'height': height ?? heightOf(preview), 'open': open ?? isOpen(preview)});
}

/// Registers the editor's own previews (once; idempotent).
void registerBuiltInPreviews() {
  PreviewProviders.register(texturePreview);
  PreviewProviders.register(materialPreview);
  PreviewProviders.register(modelPreview);
  PreviewProviders.register(rigPreview);
  PreviewProviders.register(prefabPreview);
}

/// The preview area for [asset], or nothing when no preview takes it.
class InspectorPreviewArea extends StatefulWidget {
  const InspectorPreviewArea({
    super.key,
    required this.asset,
    required this.file,
    this.title,
    this.importSettings = const {},
    this.revision,
    this.maxHeight = double.infinity,
  });

  final EditorAsset asset;

  /// The asset's file, absolute.
  final String file;
  final Map<String, Object?> importSettings;

  /// The header's name (`swatch`); the asset's file name by default.
  final String? title;

  /// Changes when the file does (its modification time): the preview
  /// reads it again.
  final Object? revision;

  /// The most the picture may take (the page keeps the rest).
  final double maxHeight;

  @override
  State<InspectorPreviewArea> createState() => _InspectorPreviewAreaState();
}

class _InspectorPreviewAreaState extends State<InspectorPreviewArea> {
  final _values = <String, Object?>{};
  double? _dragHeight;
  EditorExtensionHost? _fallbackHost;

  @override
  void didUpdateWidget(covariant InspectorPreviewArea old) {
    super.didUpdateWidget(old);
    if (old.asset.id != widget.asset.id) _values.clear();
  }

  @override
  void dispose() {
    _fallbackHost?.dispose();
    super.dispose();
  }

  /// A package's builder threw: the Console says so after this frame (a
  /// log line rebuilds the panels that show it).
  void _failed(EditorExtensionHost host, PreviewProvider provider, Object e, StackTrace stack) {
    if (provider.package == PreviewProvider.editorPackage) return;
    WidgetsBinding.instance.addPostFrameCallback((_) => host.previewFailed(provider, e, stack));
  }

  EditorExtensionHost get _host =>
      EditorExtensionScope.maybeOf(context) ?? (_fallbackHost ??= EditorExtensionHost(const []));

  @override
  Widget build(BuildContext context) {
    registerBuiltInPreviews();
    final host = _host;
    final provider = PreviewProviders.forAsset(widget.asset, packages: host.previews);
    if (provider == null) return const SizedBox.shrink();
    final preview = _Preview(
      asset: widget.asset,
      file: widget.file,
      importSettings: widget.importSettings,
      editor: host.contextFor(provider.package),
      values: _values,
      revision: widget.revision,
      onSet: () {
        if (mounted) setState(() {});
      },
    );
    final key = provider.title;
    final open = PreviewLayout.isOpen(key);
    final most = widget.maxHeight.isFinite ? math.max(PreviewLayout.minHeight, widget.maxHeight) : 4000.0;
    final height = (_dragHeight ?? PreviewLayout.heightOf(key)).clamp(PreviewLayout.minHeight, most);
    final t = context.tokens;

    Widget guarded(Widget Function() build) {
      try {
        return build();
      } catch (e, stack) {
        _failed(host, provider, e, stack);
        return _Failed(message: '$e');
      }
    }

    String? info;
    if (provider.info case final line?) {
      try {
        info = line(preview);
      } catch (e, stack) {
        _failed(host, provider, e, stack);
        info = null;
      }
    }

    return Column(
      key: const Key('preview.area'),
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // The line above the header: drag it to resize.
        MouseRegion(
          cursor: open ? SystemMouseCursors.resizeRow : MouseCursor.defer,
          child: GestureDetector(
            key: const Key('preview.resize'),
            behavior: HitTestBehavior.opaque,
            onVerticalDragStart: open ? (_) => setState(() => _dragHeight = height) : null,
            onVerticalDragUpdate: open
                ? (d) => setState(
                    () => _dragHeight = ((_dragHeight ?? height) - d.delta.dy).clamp(PreviewLayout.minHeight, most),
                  )
                : null,
            onVerticalDragEnd: open
                ? (_) {
                    PreviewLayout.set(key, height: _dragHeight);
                    setState(() => _dragHeight = null);
                  }
                : null,
            child: Container(
              height: 5,
              alignment: Alignment.topCenter,
              child: Container(height: 1, color: t.lineStrong),
            ),
          ),
        ),
        Material(
          color: t.header,
          child: SizedBox(
            height: t.row + 6,
            child: Row(
              children: [
                Expanded(
                  child: InkWell(
                    key: const Key('preview.fold'),
                    onTap: () => setState(() => PreviewLayout.set(key, open: !open)),
                    child: Padding(
                      padding: const EdgeInsets.only(left: 2, right: 4),
                      child: Row(
                        children: [
                          Icon(open ? FIcons.expandMore : FIcons.chevronRight, size: 14, color: t.textSecondary),
                          const SizedBox(width: 4),
                          Expanded(
                            child: Text(
                              widget.title ?? widget.asset.name,
                              key: const Key('preview.title'),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.bodySmall
                                  ?.copyWith(color: t.text, fontWeight: FontWeight.w600),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
                if (open && provider.toolbar != null)
                  Padding(
                    padding: const EdgeInsets.only(right: 4),
                    child: KeyedSubtree(
                      key: const Key('preview.toolbar'),
                      child: guarded(() => provider.toolbar!(context, preview)),
                    ),
                  ),
              ],
            ),
          ),
        ),
        if (open)
          SizedBox(
            height: height,
            child: ColoredBox(
              color: t.window,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Expanded(
                    child: ClipRect(
                      child: KeyedSubtree(
                        key: const Key('preview.picture'),
                        child: guarded(() => provider.build(context, preview)),
                      ),
                    ),
                  ),
                  if (info != null && info.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.fromLTRB(6, 2, 6, 3),
                      child: Text(
                        info,
                        key: const Key('preview.info'),
                        textAlign: TextAlign.center,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.bodySmall
                            ?.copyWith(color: t.textSecondary, fontFeatures: const [FontFeature.tabularFigures()]),
                      ),
                    ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _Failed extends StatelessWidget {
  const _Failed({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Text(
          'The preview failed: $message',
          key: const Key('preview.error'),
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.error),
        ),
      ),
    );
  }
}

/// A preview's [PreviewContext]: the area's shared values.
class _Preview implements PreviewContext {
  _Preview({
    required this.asset,
    required this.file,
    required this.importSettings,
    required this.editor,
    required this.values,
    required this.revision,
    required this.onSet,
  });

  @override
  final EditorAsset asset;
  @override
  final String file;
  @override
  final Map<String, Object?> importSettings;
  @override
  final EditorContext editor;
  final Map<String, Object?> values;
  final Object? revision;
  final VoidCallback onSet;

  @override
  T value<T>(String key, T initial) {
    final v = values[key];
    return v is T ? v : initial;
  }

  @override
  void set(String key, Object? value) {
    if (values[key] == value) return;
    values[key] = value;
    onSet();
  }

  @override
  Widget image(String file, {String channel = 'rgb', int mip = 0, void Function(Size size)? onSize}) =>
      PreviewImageView(
        key: ValueKey('preview.image.$file.$revision'),
        file: file,
        channel: channel,
        mip: mip,
        onSize: onSize,
      );

  @override
  Widget stage({String? asset, String? sub, Object? shape, void Function(PreviewStageStats stats)? onStats}) =>
      StageView(
        key: ValueKey('preview.stage.${asset ?? this.asset.id}.$sub.$revision'),
        asset: asset ?? this.asset.id,
        sub: sub,
        shape: shape,
        onStats: onStats,
      );
}
