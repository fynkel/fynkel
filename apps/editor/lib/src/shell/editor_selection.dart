// What the Inspector shows (blueprint item 9c, "one Inspector"): the last
// thing the user selected, wherever it was — entities in the Hierarchy or
// the Scene view, an asset or a part of one in the Project browser, a
// folder, a script or a scripts folder — whatever was selected shows. The Inspector draws the page registered for the target's kind.
//
// Selecting in one place moves the Inspector there; so does coming back to
// a panel (a click in the Hierarchy shows its selection again even when it
// did not change). The padlock pins the target: new selections leave the
// Inspector alone until it is unlocked. Pickers never write it — choosing
// an asset for a field is not selecting that asset.
//
// Item 9e: every Inspector copy has its own EditorSelection over its own
// InspectorModel, and each one follows every Project browser copy — the
// browser the user last selected in moves every unlocked Inspector.
import 'package:flutter/foundation.dart';

import '../inspector/inspector_model.dart';
import '../project_browser/project_browser_model.dart';

/// What the Inspector is showing.
sealed class InspectTarget {
  const InspectTarget();
}

/// The focused tab's selected entities (the entity page; the model knows which).
final class InspectEntities extends InspectTarget {
  const InspectEntities();

  @override
  bool operator ==(Object other) => other is InspectEntities;

  @override
  int get hashCode => (InspectEntities).hashCode;
}

/// An asset, or one of its parts (`mesh/0`, `anim/1`) when [sub] is set.
final class InspectAsset extends InspectTarget {
  const InspectAsset(this.id, {this.sub});

  final String id;
  final String? sub;

  @override
  bool operator ==(Object other) => other is InspectAsset && other.id == id && other.sub == sub;

  @override
  int get hashCode => Object.hash(id, sub);
}

/// A folder of the asset root ('' is the root).
final class InspectFolder extends InspectTarget {
  const InspectFolder(this.rel);

  final String rel;

  @override
  bool operator ==(Object other) => other is InspectFolder && other.rel == rel;

  @override
  int get hashCode => rel.hashCode;
}

/// A script under `lib/` (the Project's Scripts), by its `lib/`-relative
/// path, or a scripts folder when [folder] ('' is `lib/` itself).
final class InspectScript extends InspectTarget {
  const InspectScript(this.rel, {this.folder = false});

  final String rel;
  final bool folder;

  @override
  bool operator ==(Object other) => other is InspectScript && other.rel == rel && other.folder == folder;

  @override
  int get hashCode => Object.hash(rel, folder);
}

/// A state or a transition of an Animator Controller (ADR-0032 § 8): what
/// the Animator window selected. [state] names a state; [transition] is a
/// transition's index in the layer; neither is the layer itself.
final class InspectAnimator extends InspectTarget {
  const InspectAnimator(this.controller, this.layer, {this.state, this.transition});

  final String controller;
  final int layer;
  final String? state;
  final int? transition;

  @override
  bool operator ==(Object other) =>
      other is InspectAnimator &&
      other.controller == controller &&
      other.layer == layer &&
      other.state == state &&
      other.transition == transition;

  @override
  int get hashCode => Object.hash(controller, layer, state, transition);
}

final class EditorSelection extends ChangeNotifier {
  EditorSelection({required this.inspector, ProjectBrowserModel? browser}) {
    inspector.addListener(_onInspector);
    if (browser != null) addBrowser(browser);
    _ids = List.of(inspector.selection);
    if (_ids.isNotEmpty) _target = _latest = const InspectEntities();
  }

  final InspectorModel inspector;

  /// The Project browsers this follows, with what each last said (its lead
  /// and folder) so only a change moves the Inspector.
  final _browsers = <ProjectBrowserModel, _BrowserSeen>{};

  /// The first browser followed (the Project panel's first copy).
  ProjectBrowserModel? get browser => _browsers.keys.firstOrNull;

  /// Follows [b] too (a Project browser copy opened).
  void addBrowser(ProjectBrowserModel b) {
    if (_browsers.containsKey(b)) return;
    final seen = _BrowserSeen(b.lead, b.folder)
      ..script = b.scriptLead
      ..scriptFolder = b.inScripts ? b.scriptFolder : null;
    seen.listener = () => _onBrowser(b);
    _browsers[b] = seen;
    b.addListener(seen.listener);
  }

  /// Stops following [b] (its copy closed).
  void removeBrowser(ProjectBrowserModel b) {
    final seen = _browsers.remove(b);
    if (seen != null) b.removeListener(seen.listener);
  }

  InspectTarget? _target;
  bool _locked = false;

  /// Set while this pins or unpins the model's entities: the selection the
  /// model then reports is not a new one the user made.
  bool _pinning = false;

  /// What the Inspector would show unlocked: where it goes on unlock.
  InspectTarget? _latest;

  // What the entity source last said, so only a change moves the Inspector.
  List<String> _ids = const [];

  /// The page to show; null before anything was selected.
  InspectTarget? get target => _target;
  bool get locked => _locked;

  /// Shows [target] (a part clicked on an asset's page, Show in Inspector).
  /// An explicit request moves a locked Inspector too; the lock then holds
  /// the new target.
  void inspect(InspectTarget target) {
    final releaseEntities = _locked && _target is InspectEntities && target is! InspectEntities;
    _latest = target;
    _show(target);
    // After the page moved, so the model letting go does not read as the pinned entities going away.
    if (releaseEntities) _pinEntities(false);
  }

  /// The padlock. Locking the entity page pins those entities in the model.
  void setLocked(bool on) {
    if (on == _locked) return;
    if (on && _target == null) return;
    _locked = on;
    if (_target is InspectEntities) _pinEntities(on);
    if (!on && _latest != null) _target = _latest;
    notifyListeners();
  }

  /// What a restart brings back (item 9e): a lock on an asset, a part of
  /// one or a folder. A lock on entities is not remembered — the editor
  /// opens a project with no documents open, so there is nothing to pin.
  Map<String, Object?>? get remembered => switch (_locked ? _target : null) {
    InspectAsset(:final id, :final sub) => {'asset': id, 'sub': ?sub},
    InspectFolder(:final rel) => {'folder': rel},
    InspectScript(:final rel, :final folder) => {'script': rel, if (folder) 'scriptFolder': true},
    _ => null,
  };

  /// Restores [remembered] when what it names still exists; otherwise the
  /// Inspector starts unlocked.
  void recall(Object? json) {
    if (json is! Map) return;
    final b = browser;
    final script = json['script'];
    if (script is String && b != null) {
      final folder = json['scriptFolder'] == true;
      final exists = folder ? script.isEmpty || b.scripts.folders.contains(script) : b.scripts.files.contains(script);
      if (!exists) return;
      _target = _latest = InspectScript(script, folder: folder);
      _locked = true;
      notifyListeners();
      return;
    }
    final InspectTarget? target = switch ((json['asset'], json['sub'], json['folder'])) {
      (final String id, final Object? sub, _) when b?.entry(id) != null => InspectAsset(
        id,
        sub: sub is String ? sub : null,
      ),
      (_, _, final String rel) when rel.isEmpty || (b?.folders.contains(rel) ?? false) => InspectFolder(rel),
      _ => null,
    };
    if (target == null) return;
    _target = _latest = target;
    _locked = true;
    notifyListeners();
  }

  void _pinEntities(bool on) {
    _pinning = true;
    try {
      inspector.setLocked(on);
    } finally {
      _pinning = false;
    }
  }

  /// A panel took the keyboard: the Inspector shows that panel's selection
  /// again (a click back in the Hierarchy after browsing assets). [panel] is
  /// the panel's id; for a Project browser copy, [from] is that copy.
  void panelFocused(String panel, {ProjectBrowserModel? from}) {
    switch (panel) {
      case 'hierarchy' || 'scene':
        if (inspector.selection.isNotEmpty) _set(const InspectEntities());
      case 'project':
        final b = from ?? browser;
        final script = b?.scriptLead;
        final lead = b?.lead;
        if (b != null && b.inScripts && script != null) {
          _set(InspectScript(script));
        } else if (lead != null) {
          _set(InspectAsset(lead, sub: b?.leadPart));
        }
    }
  }

  /// A source moved: remembered, and shown unless the Inspector is locked.
  void _set(InspectTarget target) {
    _latest = target;
    if (!_locked) _show(target);
  }

  void _show(InspectTarget target) {
    if (target == _target) return;
    _target = target;
    notifyListeners();
  }

  void _onInspector() {
    // A pinned entity page whose entities (or tab) went away lets go.
    if (_locked && _target is InspectEntities && !inspector.locked) {
      _locked = false;
      notifyListeners();
    }
    final ids = inspector.selection;
    if (listEquals(ids, _ids)) return;
    _ids = List.of(ids);
    if (ids.isNotEmpty && !_pinning) _set(const InspectEntities());
  }

  void _onBrowser(ProjectBrowserModel b) {
    final seen = _browsers[b];
    if (seen == null) return;
    final lead = b.lead;
    final folder = b.folder;
    final part = b.leadPart;
    final leadMoved = lead != seen.lead || part != seen.part;
    final folderMoved = folder != seen.folder;
    seen.lead = lead;
    seen.folder = folder;
    seen.part = part;
    // The Scripts root: the script, or the scripts folder, shown.
    final script = b.scriptLead;
    final scriptFolder = b.inScripts ? b.scriptFolder : null;
    final scriptMoved = script != seen.script;
    final scriptFolderMoved = scriptFolder != seen.scriptFolder;
    seen.script = script;
    seen.scriptFolder = scriptFolder;
    // A deleted asset or folder cannot stay on the page.
    final t = _target;
    if (t is InspectAsset && b.entry(t.id) == null) {
      _locked = false;
      _target = _latest = lead != null ? InspectAsset(lead) : null;
      notifyListeners();
      return;
    }
    if (t is InspectFolder && t.rel.isNotEmpty && !b.folders.contains(t.rel)) {
      _locked = false;
      _target = _latest = null;
      notifyListeners();
      return;
    }
    if (t is InspectScript && !(t.folder ? t.rel.isEmpty || b.scripts.folders.contains(t.rel) : b.scripts.files.contains(t.rel))) {
      _locked = false;
      _target = _latest = null;
      notifyListeners();
      return;
    }
    if (scriptMoved && script != null) {
      _set(InspectScript(script));
      return;
    }
    if (scriptFolderMoved && scriptFolder != null) {
      _set(InspectScript(scriptFolder, folder: true));
      return;
    }
    if (scriptMoved && _latest is InspectScript && !(_latest as InspectScript).folder) {
      // The script selection was cleared: the page empties too.
      _latest = null;
      if (!_locked) {
        _target = null;
        notifyListeners();
      }
      return;
    }
    if (leadMoved && lead != null) {
      _set(InspectAsset(lead, sub: part));
    } else if (folderMoved) {
      _set(InspectFolder(folder));
    } else if (leadMoved && _latest is InspectAsset) {
      // The browser's selection was cleared: the asset page empties too.
      _latest = null;
      if (!_locked) {
        _target = null;
        notifyListeners();
      }
    }
  }

  @override
  void dispose() {
    inspector.removeListener(_onInspector);
    for (final e in _browsers.entries) {
      e.key.removeListener(e.value.listener);
    }
    _browsers.clear();
    super.dispose();
  }
}

final class _BrowserSeen {
  _BrowserSeen(this.lead, this.folder);

  String? lead;
  String? folder;
  String? part;
  String? script;
  String? scriptFolder;
  late VoidCallback listener;
}
