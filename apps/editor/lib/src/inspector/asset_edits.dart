// The Inspector's edits of a document asset — a material or a volume
// profile (docs/unity-parity.md, Tier 2): as in Unity, such an asset is
// edited where it is shown — the Inspector — and each change is one
// transaction on its document, saved at once (there is no Save for a
// material or a profile). ⌘Z / ⇧⌘Z undo and redo those changes while the
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
  /// `material` or `volume`); null draws the cooked asset again. The shell
  /// points it at the Scene view.
  void Function(String kind, String id, Map<String, Object?>? fields)? preview;

  /// The asset an Inspector page shows (what ⌘Z undoes while the
  /// Inspector holds the keyboard); set and cleared by the page.
  String? shown;

  /// Why the last change was refused, until the next one.
  String? lastError;

  final _documents = <String, EditorDocument>{};

  bool get canEdit => project.readOnly == null;

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
    if (doc == null || !canEdit || changes.isEmpty) return false;
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
    preview?.call(doc.kind, id, {...doc.root(), ...?changes});
  }

  void _show(String id, EditorDocument doc) => preview?.call(doc.kind, id, doc.root());

  /// The asset ⌘Z undoes now (redo: ⇧⌘Z): the one shown, when
  /// [inspectorFocused] and it has something to undo (redo).
  EditorDocument? undoTarget({required bool inspectorFocused, bool redo = false}) {
    final id = shown;
    if (id == null || !inspectorFocused) return null;
    final doc = _documents[id];
    if (doc == null || doc.isClosed) return null;
    return (redo ? doc.canRedo : doc.canUndo) ? doc : null;
  }

  /// "Material" or "Volume Profile": what the Edit menu's Undo names.
  static String noun(EditorDocument doc) => doc.kind == 'volume' ? 'Volume Profile' : 'Material';

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
