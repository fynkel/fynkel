// The typed Dart surface over the authoring ABI (blueprint §19.2e item 1,
// ADR-0016): one AuthoringSession per editor window, one DocumentHandle per
// open tab. Every call crosses the ABI once — text in, one JSON result out —
// and is mapped from the status to a return value or an AuthoringException.
//
// Change notification is a pure-Dart `Stream` (this package is
// Flutter-independent like package:fynkel); the editor app wraps it in
// whatever its widgets listen to.

import 'dart:async';
import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'diagnostic.dart';
import 'models.dart';
import 'native/fynkel_authoring_bindings.g.dart' as n;

/// The authoring ABI version this package was written against.
const int authoringAbiVersion = n.FYNKEL_AUTHORING_ABI_VERSION;

/// The status every ABI call answers with (fynkel.h `FynkelStatus`).
enum AuthoringStatus {
  ok,
  commandsRejected,
  invalidArgument,
  protocol,
  poisoned,
  panic,
  outOfResources,
  incompatible,
  loadFailed;

  static AuthoringStatus _of(n.FynkelStatus s) => switch (s) {
        n.FynkelStatus.FYNKEL_STATUS_OK => ok,
        n.FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED => commandsRejected,
        n.FynkelStatus.FYNKEL_STATUS_INVALID_ARGUMENT => invalidArgument,
        n.FynkelStatus.FYNKEL_STATUS_PROTOCOL => protocol,
        n.FynkelStatus.FYNKEL_STATUS_POISONED => poisoned,
        n.FynkelStatus.FYNKEL_STATUS_PANIC => panic,
        n.FynkelStatus.FYNKEL_STATUS_OUT_OF_RESOURCES => outOfResources,
        n.FynkelStatus.FYNKEL_STATUS_INCOMPATIBLE => incompatible,
        n.FynkelStatus.FYNKEL_STATUS_LOAD_FAILED => loadFailed,
      };
}

/// A call the session refused or could not complete. [diagnostics] is the
/// ADR-0014 list the ABI wrote (`FYN-E####` editor rules, `FYN-T####`
/// session and ABI, `FYN-D####`/`FYN-P####` documents and projects).
final class AuthoringException implements Exception {
  AuthoringException(this.operation, this.status, this.diagnostics);

  /// What was attempted, e.g. `openProject`.
  final String operation;
  final AuthoringStatus status;
  final List<Diagnostic> diagnostics;

  /// The first diagnostic's code, or null when the ABI wrote none.
  String? get code => diagnostics.isEmpty ? null : diagnostics.first.code;

  /// A stale `baseRevision` (FYN-E0001): the document moved on.
  bool get isConflict => code == 'FYN-E0001';

  /// The project is read-only (FYN-E0007).
  bool get isReadOnly => code == 'FYN-E0007';

  /// Nothing to undo or redo (FYN-E0009).
  bool get isNothingToReplay => code == 'FYN-E0009';

  /// The native session panicked (FYN-T0005) and is poisoned: dispose it and
  /// open a new one.
  bool get isPoisoned => status == AuthoringStatus.panic || status == AuthoringStatus.poisoned;

  @override
  String toString() =>
      'AuthoringException($operation, ${status.name})${diagnostics.isEmpty ? '' : ': ${diagnostics.join('; ')}'}';
}

/// What changed in a document, on [AuthoringSession.events] and
/// [DocumentHandle.changes].
enum DocumentChangeKind { applied, undone, redone, saved, reloaded }

/// Something happened in the session — the editor's panels rebuild from these.
sealed class AuthoringEvent {
  const AuthoringEvent();
}

final class ProjectOpened extends AuthoringEvent {
  const ProjectOpened(this.project);
  final ProjectInfo project;
}

final class ProjectClosed extends AuthoringEvent {
  const ProjectClosed();
}

final class DocumentOpened extends AuthoringEvent {
  const DocumentOpened(this.document);
  final DocumentHandle document;
}

/// The document's content or state moved; [state] is the state after it.
final class DocumentChanged extends AuthoringEvent {
  const DocumentChanged(this.document, this.kind, this.state, {this.applied, this.replayed});
  final DocumentHandle document;
  final DocumentChangeKind kind;
  final DocumentState state;

  /// The transaction, for [DocumentChangeKind.applied].
  final Applied? applied;

  /// The replayed transaction, for undo and redo.
  final Replayed? replayed;
}

final class DocumentClosed extends AuthoringEvent {
  const DocumentClosed(this.handle);
  final int handle;
}

/// The asset database was rescanned (a save, an explicit rescan).
final class AssetsChanged extends AuthoringEvent {
  const AssetsChanged(this.report);
  final RescanReport report;
}

/// One authoring session: a project and its open documents, behind the
/// native `FynkelAuthoringSession`. Not thread-safe; drive it from one
/// isolate. [dispose] frees it; a session that is garbage-collected
/// undisposed is freed by a finalizer.
final class AuthoringSession implements Finalizable {
  /// Creates a session with no project. Throws [StateError] when the loaded
  /// runtime's authoring ABI is not [authoringAbiVersion] (the app's
  /// `hooks.user_defines.fynkel.features` must list `authoring`).
  AuthoringSession() {
    final v = n.fynkel_authoring_abi_version();
    if (v != authoringAbiVersion) {
      throw StateError('authoring ABI $v in the loaded runtime, this package expects $authoringAbiVersion');
    }
    _ptr = n.fynkel_authoring_session_create();
    if (_ptr == nullptr) throw StateError('fynkel_authoring_session_create failed');
    _finalizer.attach(this, _ptr.cast(), detach: this);
  }

  static final _finalizer = NativeFinalizer(Native.addressOf<NativeFunction<Void Function(Pointer<Void>)>>(_destroy));

  /// The authoring ABI version of the loaded runtime.
  static int get loadedAbiVersion => n.fynkel_authoring_abi_version();

  late final Pointer<n.FynkelAuthoringSession> _ptr;
  final _events = StreamController<AuthoringEvent>.broadcast(sync: true);
  final _documents = <int, DocumentHandle>{};
  ProjectInfo? _project;
  bool _disposed = false;

  /// Everything that happens in the session, in order, synchronously after
  /// the call that caused it returns its value.
  Stream<AuthoringEvent> get events => _events.stream;

  /// The open project, or null.
  ProjectInfo? get project => _project;

  /// The open documents by handle.
  Iterable<DocumentHandle> get documents => _documents.values;

  bool get isDisposed => _disposed;

  /// The native session caught a panic; every call now throws. Dispose it.
  bool get isPoisoned => !_disposed && n.fynkel_authoring_is_poisoned(_ptr) != 0;

  /// Opens the project at [dir] (closing any open project and its documents
  /// first): the project file, its lock (a newer Fynkel's lock makes it
  /// read-only), schemas, the asset database and the behaviour metadata.
  /// Throws [AuthoringException] (`loadFailed`) when there is none.
  ProjectInfo openProject(String dir) {
    _closeAllHandles();
    final r = _text('openProject', dir, (p, len) => n.fynkel_authoring_open_project(_ptr, p, len));
    final project = ProjectInfo.fromJson(_mapOf(r['project']));
    _project = project;
    _events.add(ProjectOpened(project));
    return project;
  }

  /// Closes the project and every document; their handles are dead.
  void closeProject() {
    _check();
    _closeAllHandles();
    _call('closeProject', () => n.fynkel_authoring_close_project(_ptr));
    _project = null;
    _events.add(const ProjectClosed());
  }

  /// Re-reads the project's description (`project` query) after something
  /// changed it (a rescan, a save).
  ProjectInfo refreshProject() {
    final p = ProjectInfo.fromJson(query('project'));
    _project = p;
    return p;
  }

  /// Opens a scene, prefab or material by path (absolute, or relative to the
  /// project directory) or by asset id; a document already open returns its
  /// existing handle. Throws [AuthoringException] (`loadFailed`) for a
  /// missing file or id, a non-document, a kind the editor does not edit.
  DocumentHandle openDocument(String pathOrId) {
    _check();
    final out = calloc<Uint32>();
    try {
      final r = _text('openDocument', pathOrId, (p, len) => n.fynkel_authoring_open_document(_ptr, p, len, out));
      final handle = out.value;
      final state = DocumentState.fromJson(_mapOf(r['state']));
      final existing = _documents[handle];
      if (existing != null) {
        existing._state = state;
        return existing;
      }
      final doc = DocumentHandle._(this, handle, state);
      _documents[handle] = doc;
      _events.add(DocumentOpened(doc));
      return doc;
    } finally {
      calloc.free(out);
    }
  }

  /// A read-only query by [kind] (`project`, `documents`, `document`,
  /// `inspect`, `validate`, `validateProject`, `assets`, `asset`,
  /// `dependents`, `schemas`, `schema`, `metadata`, `commands`) with its
  /// arguments; returns the `result` value. Throws [AuthoringException]
  /// (`invalidArgument`) for an unknown kind or a missing argument.
  Object? queryValue(String kind, [Map<String, Object?> args = const {}]) {
    _check();
    final kindBytes = utf8.encode(kind);
    final argBytes = utf8.encode(jsonEncode(args));
    final buf = calloc<Uint8>(kindBytes.length + argBytes.length + 1);
    try {
      buf.asTypedList(kindBytes.length).setAll(0, kindBytes);
      final argPtr = buf + kindBytes.length;
      argPtr.asTypedList(argBytes.length).setAll(0, argBytes);
      final r = _call('query $kind', () => n.fynkel_authoring_query(_ptr, buf, kindBytes.length, argPtr, argBytes.length));
      return r['result'];
    } finally {
      calloc.free(buf);
    }
  }

  /// [queryValue] for the kinds whose result is an object.
  Map<String, Object?> query(String kind, [Map<String, Object?> args = const {}]) => _mapOf(queryValue(kind, args));

  /// The command names `apply` accepts.
  List<String> commands() => _strings(queryValue('commands'));

  /// Every open document's state, from the session.
  List<DocumentState> documentStates() =>
      [for (final s in queryValue('documents') as List? ?? const []) DocumentState.fromJson(_mapOf(s))];

  /// `fynkel validate --json` over the project.
  ProjectReport validateProject() => ProjectReport.fromJson(query('validateProject'));

  /// The asset entries; [kind] `document` or `source`, [document] a document
  /// kind name such as `scene` to narrow further.
  List<Map<String, Object?>> assets({String? kind, String? document}) => [
        for (final a in queryValue('assets', {'kind': ?kind, 'document': ?document})
                as List? ??
            const [])
          _mapOf(a)
      ];

  /// One resolved asset entry; throws (`invalidArgument`, FYN-A0004) when unknown.
  Map<String, Object?> asset(String id, {String? sub}) => query('asset', {'asset': id, 'sub': ?sub});

  /// Every reference to [id]: `{ document, path, pointer, asset, sub? }`.
  List<Map<String, Object?>> dependents(String id) =>
      [for (final d in queryValue('dependents', {'asset': id}) as List? ?? const []) _mapOf(d)];

  /// `{ components: [...], behaviours: [...] }` — every schema with its
  /// `x-fynkel` display hints.
  Map<String, Object?> schemas() => query('schemas');

  /// One component or behaviour schema by type id.
  Map<String, Object?> schema(String type) => query('schema', {'type': type});

  /// The committed behaviour metadata (`lib/generated/behaviours.g.json`), or null.
  Map<String, Object?>? metadata() {
    final v = queryValue('metadata');
    return v == null ? null : _mapOf(v);
  }

  /// Rescans the asset database; [paths] (absolute, or relative to the
  /// project directory) are the files whose reference-index entries are
  /// refreshed — what a file watcher reports — or empty for everything.
  RescanReport rescan([Iterable<String> paths = const []]) {
    _check();
    final list = paths.toList();
    final r = list.isEmpty
        ? _call('rescan', () => n.fynkel_authoring_rescan(_ptr, nullptr, 0))
        : _text('rescan', jsonEncode(list), (p, len) => n.fynkel_authoring_rescan(_ptr, p, len));
    final report = RescanReport.fromJson(r);
    _events.add(AssetsChanged(report));
    return report;
  }

  /// Frees the native session and its documents (unsaved edits are lost —
  /// ask first). Every later call throws [StateError].
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    for (final d in _documents.values) {
      d._closed = true;
    }
    _documents.clear();
    _finalizer.detach(this);
    n.fynkel_authoring_session_destroy(_ptr);
    _events.close();
  }

  // ---- plumbing --------------------------------------------------------

  void _check() {
    if (_disposed) throw StateError('AuthoringSession is disposed');
  }

  void _closeAllHandles() {
    for (final d in _documents.values) {
      d._closed = true;
      _events.add(DocumentClosed(d.handle));
    }
    _documents.clear();
  }

  /// Runs one ABI call and reads its JSON result. `commandsRejected` is
  /// returned as the record when [rejectedIsResult] (apply), thrown
  /// otherwise; every other non-OK status throws.
  Map<String, Object?> _call(String op, n.FynkelStatus Function() f, {bool rejectedIsResult = false}) {
    _check();
    final status = AuthoringStatus._of(f());
    final result = _result();
    if (status == AuthoringStatus.ok || (rejectedIsResult && status == AuthoringStatus.commandsRejected)) {
      return result;
    }
    throw AuthoringException(op, status, Diagnostic.listOf(result['diagnostics']));
  }

  Map<String, Object?> _text(String op, String text, n.FynkelStatus Function(Pointer<Uint8>, int) f,
      {bool rejectedIsResult = false}) {
    final bytes = utf8.encode(text);
    final buf = calloc<Uint8>(bytes.length + 1);
    try {
      buf.asTypedList(bytes.length).setAll(0, bytes);
      return _call(op, () => f(buf, bytes.length), rejectedIsResult: rejectedIsResult);
    } finally {
      calloc.free(buf);
    }
  }

  Map<String, Object?> _result() {
    final view = n.fynkel_authoring_result(_ptr);
    if (view.length == 0) return const {};
    final decoded = jsonDecode(utf8.decode(view.data.asTypedList(view.length)));
    return decoded is Map<String, Object?> ? decoded : {'result': decoded};
  }
}

// The destroy entry point once more, untyped, so its address can back the
// NativeFinalizer (the generated binding takes the typed opaque pointer).
@Native<Void Function(Pointer<Void>)>(symbol: 'fynkel_authoring_session_destroy', assetId: 'package:fynkel/fynkel_native')
external void _destroy(Pointer<Void> session);

Map<String, Object?> _mapOf(Object? v) => v is Map<String, Object?> ? v : const {};
List<String> _strings(Object? v) => v is List ? [for (final s in v) if (s is String) s] : const [];

/// One open document: an `Editor` in the native session behind a handle.
/// Obtained from [AuthoringSession.openDocument]; dead after [close],
/// [AuthoringSession.closeProject] or [AuthoringSession.dispose].
final class DocumentHandle {
  DocumentHandle._(this.session, this.handle, this._state);

  final AuthoringSession session;

  /// The session's integer handle (never reused within a session).
  final int handle;
  DocumentState _state;
  bool _closed = false;

  /// The state after the last call on this document.
  DocumentState get state => _state;
  bool get isClosed => _closed;

  /// Changes to this document only, from [AuthoringSession.events].
  Stream<DocumentChanged> get changes =>
      session.events.where((e) => e is DocumentChanged && e.document == this).cast<DocumentChanged>();

  /// Applies ONE transaction of [commands] (see `Command`). [baseRevision]
  /// names the revision the caller edited from — a stale one is refused
  /// (FYN-E0001) and nothing is touched; [transactionId] may be chosen by
  /// the caller. Returns the record whether or not it was committed (read
  /// [Applied.ok]); throws [AuthoringException] for a malformed request
  /// (not JSON), a dead handle, a read-only project or a panic.
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision, String? transactionId}) => applyRequest({
        'transactionId': ?transactionId,
        'baseRevision': ?baseRevision,
        'commands': commands,
      });

  /// [apply] with the raw request: a JSON array of commands, or
  /// `{ transactionId?, baseRevision?, commands }`.
  Applied applyRequest(Object request) {
    _check();
    final r = session._text(
      'apply',
      jsonEncode(request),
      (p, len) => n.fynkel_authoring_apply(session._ptr, handle, p, len),
      rejectedIsResult: true,
    );
    final applied = Applied.fromJson(r);
    if (applied.state != null) _state = applied.state!;
    if (applied.ok) session._events.add(DocumentChanged(this, DocumentChangeKind.applied, _state, applied: applied));
    return applied;
  }

  /// Replays the last committed transaction's inverse edits, byte-exact.
  /// Throws (`commandsRejected`, FYN-E0009) when there is nothing to undo.
  Replayed undo() => _replay('undo', DocumentChangeKind.undone, () => n.fynkel_authoring_undo(session._ptr, handle));

  /// Replays the last undone transaction forward. Throws (FYN-E0009) when
  /// there is nothing to redo.
  Replayed redo() => _replay('redo', DocumentChangeKind.redone, () => n.fynkel_authoring_redo(session._ptr, handle));

  Replayed _replay(String op, DocumentChangeKind kind, n.FynkelStatus Function() f) {
    _check();
    final t = Replayed.fromJson(session._call(op, f));
    _state = t.state;
    session._events.add(DocumentChanged(this, kind, _state, replayed: t));
    return t;
  }

  /// Writes the document canonically where it was read from, or to [path]
  /// (absolute, or relative to the process's working directory), which
  /// becomes its path; the asset database is rescanned for it. Returns the
  /// written path. Throws (`loadFailed`, FYN-E0011) when it cannot be
  /// written, (`commandsRejected`, FYN-E0007) when the project is read-only.
  String save({String? path}) {
    _check();
    final r = path == null
        ? session._call('save', () => n.fynkel_authoring_save(session._ptr, handle, nullptr, 0))
        : session._text('save', path, (p, len) => n.fynkel_authoring_save(session._ptr, handle, p, len));
    _state = DocumentState.fromJson(_mapOf(r['state']));
    session._events.add(DocumentChanged(this, DocumentChangeKind.saved, _state));
    return r['path'] as String? ?? _state.path;
  }

  /// Re-reads the document from disk: history dropped, edits lost ("revert
  /// to saved", or the silent reload after an external change with no local
  /// edits).
  DocumentState reload() {
    _check();
    final r = session._call('reload', () => n.fynkel_authoring_reload_document(session._ptr, handle));
    _state = DocumentState.fromJson(_mapOf(r['state']));
    session._events.add(DocumentChanged(this, DocumentChangeKind.reloaded, _state));
    return _state;
  }

  /// Drops the document and its history (unsaved edits are lost — ask first).
  void close() {
    if (_closed) return;
    session._call('closeDocument', () => n.fynkel_authoring_close_document(session._ptr, handle));
    _closed = true;
    session._documents.remove(handle);
    session._events.add(DocumentClosed(handle));
  }

  /// The state from the session (not the cached one).
  DocumentState refreshState() => _state = DocumentState.fromJson(session.query('document', {'handle': handle, 'root': false}));

  /// The whole document as JSON (`root`), the envelope included — what the
  /// Hierarchy reads (`entities` by id with `name`, `parent`, `children`,
  /// `components`, `prefab?`).
  Map<String, Object?> root() {
    _check();
    final r = session.query('document', {'handle': handle, 'root': true});
    _state = DocumentState.fromJson(r);
    return _mapOf(r['root']);
  }

  /// The typed view `fynkel scene|prefab inspect --json` prints (a material:
  /// shader, alphaMode, alphaCutoff, doubleSided, fields), plus
  /// `diagnostics` of the document now.
  Map<String, Object?> inspect() {
    _check();
    return session.query('inspect', {'handle': handle});
  }

  /// The document's diagnostics now, asset references checked.
  List<Diagnostic> validate() {
    _check();
    return Diagnostic.listOf(session.query('validate', {'handle': handle})['diagnostics']);
  }

  void _check() {
    session._check();
    if (_closed) throw StateError('document handle $handle is closed');
  }

  @override
  String toString() => 'DocumentHandle($handle, ${_state.kind} ${_state.name})';
}
