// A row's, a picker's or an Inspector page's preview: the decoded texture, the material
// swatch, or the kind's icon.
import 'dart:io';

import 'package:flutter/material.dart';

import '../inspector/field_widgets.dart' show assetKindIcon;
import 'project_browser_model.dart';

class AssetPreviewWidget extends StatelessWidget {
  const AssetPreviewWidget({super.key, required this.preview, this.size = 20});

  final AssetPreview preview;
  final double size;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final radius = BorderRadius.circular(size >= 40 ? 6 : 3);
    return switch (preview) {
      // The texture's icon until the picture is decoded (a field's small
      // thumbnail is never a blank gap before the name), and a hairline
      // round it so a dark texture still shows on a dark field.
      TexturePreview(:final file) => Container(
        foregroundDecoration: BoxDecoration(
          borderRadius: radius,
          border: Border.all(color: theme.dividerColor),
        ),
        child: ClipRRect(
          borderRadius: radius,
          child: Image.file(
            File(file),
            key: ValueKey('preview.texture.$file'),
            width: size,
            height: size,
            fit: BoxFit.cover,
            cacheWidth: (size * 2).round(),
            filterQuality: FilterQuality.medium,
            frameBuilder: (context, child, frame, sync) => frame == null && !sync
                ? SizedBox.square(
                    dimension: size,
                    child: Icon(assetKindIcon('texture'), size: size * .8, color: theme.hintColor),
                  )
                : child,
            errorBuilder: (_, _, _) => Icon(assetKindIcon('texture'), size: size * .8, color: theme.hintColor),
          ),
        ),
      ),
      SwatchPreview(:final color) => Container(
        key: const Key('preview.swatch'),
        width: size,
        height: size,
        decoration: BoxDecoration(
          color: color,
          borderRadius: radius,
          border: Border.all(color: theme.dividerColor),
        ),
      ),
      IconPreview(:final kind) => SizedBox(
        width: size,
        height: size,
        child: Icon(assetKindIcon(kind), size: size * .8, color: theme.hintColor),
      ),
    };
  }
}
