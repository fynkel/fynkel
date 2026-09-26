// The Inspector's edits of a document asset — a material, a volume
// profile or a physics material (docs/unity-parity.md, Tiers 2 and 3): as
// in Unity, such an asset is edited where it is shown — the Inspector —
// and each change is one transaction on its document, saved at once
// (there is no Save for a material or a profile). ⌘Z / ⇧⌘Z undo and redo those changes while the
// Inspector or the Project holds the keyboard and the Inspector shows the
// asset; the shell routes them here ([undoTarget]).
//
// The Scene view always draws what the document holds, a moment before
// the cache catches up: every change, undo and redo hands the fields to
// [preview], and so does each step of a colour being picked or a label
// being dragged ([show]) — those write nothing.
import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/editor_document.dart';
import '../project/editor_project.dart';

/// The shader whose fields the material Inspector edits.
const pbrShader = 'fynkel.pbr@1';

/// The value of [AssetEdits.edit] that removes a member.
const unset = Object();

final class AssetEdits extends ChangeNotifier {
  AssetEdits(this.project);

  final EditorProject project;

  /// Draws an asset's fields in the Scene view (its document [kind]:
  /// `material` or `volume`; a physics material draws nothing); null draws
  /// the cooked asset again. The shell
  /// points it at the Scene view.
  void Function(String kind, String id, Map<String, Object?>? fields)? preview;

  /// Also told every drawn change, after [preview]: the Inspector's own
  /// 3D preview of the asset (ADR-0030), which draws with no Scene view.
  final watchers = <void Function(String kind, String id, Map<String, Object?> fields)>{};

  /// The fields last drawn for [id] since the editor started — an asset
  /// edited, undone or being picked — else null (the cooked asset is
  /// what it holds).
  Map<String, Object?>? drawn(String id) => _drawn[id];
  final _drawn = <String, Map<String, Object?>>{};

  /// The asset an Inspector page shows (what ⌘Z undoes while the
  /// Inspector holds the keyboard); set and cleared by the page.
  String? shown;

  /// Why the last change was refused, until the next one.
  String? lastError;

  final _documents = <String, EditorDocument>{};

  bool get canEdit => project.readOnly == null;

  /// Whether the asset [id] can be changed: the project can be, and it is
  /// not a package's that pub keeps (ADR-0029: Copy into Project first).
  bool canEditAsset(String id) => canEdit && readOnlyReason(id) == null;

  /// Why [id] cannot be changed although the project can; null when it can.
  String? readOnlyReason(String id) {
    for (final a in project.assets()) {
      if (a.id == id) return a.readOnly;
    }
    return null;
  }

  /// The asset's document, opened on first use and again after its tab
  /// closed it; null (with [lastError]) when it cannot be opened.
  EditorDocument? document(String id) {
    final open = _documents[id];
    if (open != null && !open.isClosed) return open;
    try {
      return _documents[id] = project.openDocument(id);
    } on AuthoringException catch (e) {
      lastError = '$e';
      return null;
    }
  }

  /// The asset's fields as its document holds them.
  Map<String, Object?>? fields(String id) => document(id)?.root();

  /// Sets [changes] (top-level field → value; null unbinds a map) in one
  /// undo step and saves. False, with [lastError], when refused.
  bool set(String id, Map<String, Object?> changes) =>
      edit(id, {for (final c in changes.entries) '/${c.key}': c.value});

  /// Sets each pointer of [changes] to its value, or removes it when the
  /// value is [unset], in one undo step, and saves. False, with
  /// [lastError], when refused.
  bool edit(String id, Map<String, Object?> changes) {
    final doc = document(id);
    if (doc == null || !canEditAsset(id) || changes.isEmpty) return false;
    try {
      doc.apply([
        for (final c in changes.entries)
          identical(c.value, unset) ? Command.unsetDocumentField(c.key) : Command.setDocumentField(c.key, c.value),
      ]);
      doc.save();
      lastError = null;
    } on AuthoringException catch (e) {
      lastError = '$e';
      _show(id, doc);
      notifyListeners();
      return false;
    }
    _show(id, doc);
    notifyListeners();
    return true;
  }

  /// The Scene view draws [id] with [changes] (top-level fields) over its
  /// fields, nothing written; null draws the document's fields again.
  void show(String id, Map<String, Object?>? changes) {
    final doc = document(id);
    if (doc == null) return;
    _draw(doc.kind, id, {...doc.root(), ...?changes});
  }

  void _show(String id, EditorDocument doc) => _draw(doc.kind, id, doc.root());

  void _draw(String kind, String id, Map<String, Object?> fields) {
    _drawn[id] = fields;
    preview?.call(kind, id, fields);
    for (final w in List.of(watchers)) {
      w(kind, id, fields);
    }
  }

  /// The asset ⌘Z undoes now (redo: ⇧⌘Z): the one shown, when
  /// [inspectorFocused] and it has something to undo (redo).
  EditorDocument? undoTarget({required bool inspectorFocused, bool redo = false}) {
    final id = shown;
    if (id == null || !inspectorFocused) return null;
    final doc = _documents[id];
    if (doc == null || doc.isClosed) return null;
    return (redo ? doc.canRedo : doc.canUndo) ? doc : null;
  }

  /// "Material", "Volume Profile" or "Physics Material": what the Edit
  /// menu's Undo names.
  static String noun(EditorDocument doc) => switch (doc.kind) {
    'volume' => 'Volume Profile',
    'physics_material' => 'Physics Material',
    'data' => 'Data Asset',
    _ => 'Material',
  };

  bool undo(EditorDocument doc, {bool redo = false}) {
    final id = _documents.entries.where((e) => e.value == doc).firstOrNull?.key;
    if (id == null) return false;
    try {
      redo ? doc.redo() : doc.undo();
      doc.save();
      lastError = null;
    } on AuthoringException catch (e) {
      lastError = '$e';
    }
    _show(id, doc);
    notifyListeners();
    return true;
  }
}
