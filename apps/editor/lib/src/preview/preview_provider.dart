// The Inspector's previews (ADR-0030): one registry of providers, each
// saying which assets it takes and how to draw them. The editor's own
// (texture, and the 3D ones to come) are registered the same way a
// package's `editor.preview(…)` is, so none of them has a private path;
// a package's comes first, so it can take over the editor's for its own
// data.
import 'package:flutter/widgets.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

/// One preview: the assets it takes and its three builders.
@immutable
class PreviewProvider {
  const PreviewProvider({
    required this.package,
    required this.build,
    this.kind,
    this.dataType,
    this.accepts,
    this.toolbar,
    this.info,
    this.name,
  });

  /// `fynkel` for the editor's own.
  final String package;

  /// Takes the assets of this kind (`kindOf`: `texture`, `material`, …).
  final String? kind;

  /// Takes the data assets of this type (`game.CoinKind`).
  final String? dataType;
  final bool Function(EditorAsset asset)? accepts;
  final Widget Function(BuildContext context, PreviewContext preview) build;
  final Widget Function(BuildContext context, PreviewContext preview)? toolbar;
  final String? Function(PreviewContext preview)? info;

  /// How the Console and the preferences name it; by default what it takes.
  final String? name;

  static const editorPackage = 'fynkel';

  String get title => name ?? [?kind, ?dataType, if (kind == null && dataType == null) 'custom'].join(' ');

  /// Whether it takes [asset]: every condition it names holds.
  bool takes(EditorAsset asset) {
    if (kind != null && asset.kind != kind) return false;
    if (dataType != null && asset.dataType != dataType) return false;
    if (accepts case final test?) {
      try {
        return test(asset);
      } catch (_) {
        return false;
      }
    }
    return true;
  }
}

/// The editor's own previews, after the packages'.
abstract final class PreviewProviders {
  static final _builtIn = <PreviewProvider>[];

  /// Adds one of the editor's own (at start, from `registerBuiltInPreviews`).
  static void register(PreviewProvider provider) {
    _builtIn.removeWhere((p) => p.title == provider.title);
    _builtIn.add(provider);
  }

  static Iterable<PreviewProvider> get builtIn => _builtIn;

  /// The preview for [asset]: the first of [packages] that takes it, else
  /// the editor's own; null when none does.
  static PreviewProvider? forAsset(EditorAsset asset, {Iterable<PreviewProvider> packages = const []}) {
    for (final p in packages) {
      if (p.takes(asset)) return p;
    }
    for (final p in _builtIn) {
      if (p.takes(asset)) return p;
    }
    return null;
  }
}
