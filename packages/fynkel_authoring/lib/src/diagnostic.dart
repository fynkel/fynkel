// The ADR-0014 diagnostic record as the authoring ABI writes it in JSON:
// code, severity, message, and where it points.

/// Severity of a [Diagnostic]; `error` is what makes a result `ok: false`.
enum Severity {
  error,
  warning,
  info;

  static Severity parse(Object? v) => switch (v) {
        'error' => Severity.error,
        'warning' => Severity.warning,
        'info' => Severity.info,
        _ => Severity.error,
      };
}

/// One diagnostic from the session: a stable `FYN-XNNNN` code, a severity,
/// a message, and optionally the document, JSON pointer, id, line/column and
/// a suggestion. `related` carries nested diagnostics (a prefab's own
/// errors under the instance that names it).
final class Diagnostic {
  const Diagnostic({
    required this.code,
    required this.severity,
    required this.message,
    this.document,
    this.pointer,
    this.id,
    this.line,
    this.column,
    this.suggestion,
    this.related = const [],
  });

  factory Diagnostic.fromJson(Map<String, Object?> j) => Diagnostic(
        code: j['code'] as String? ?? 'FYN-????',
        severity: Severity.parse(j['severity']),
        message: j['message'] as String? ?? '',
        document: j['document'] as String?,
        pointer: j['pointer'] as String?,
        id: j['id'] as String?,
        line: (j['line'] as num?)?.toInt(),
        column: (j['column'] as num?)?.toInt(),
        suggestion: j['suggestion'] as String?,
        related: listOf(j['related']),
      );

  /// Parses a JSON array of diagnostic records; anything else is empty.
  static List<Diagnostic> listOf(Object? v) => v is List
      ? [for (final d in v) if (d is Map<String, Object?>) Diagnostic.fromJson(d)]
      : const [];

  /// Stable code, e.g. `FYN-E0001` (a stale revision).
  final String code;
  final Severity severity;
  final String message;

  /// Path of the document the diagnostic is about, when it has one.
  final String? document;

  /// JSON pointer into that document.
  final String? pointer;

  /// The entity, component or asset id concerned.
  final String? id;
  final int? line;
  final int? column;
  final String? suggestion;
  final List<Diagnostic> related;

  bool get isError => severity == Severity.error;

  @override
  String toString() {
    final where = [
      if (document != null) document,
      if (line != null) ':$line${column != null ? ':$column' : ''}',
      if (pointer != null) ' $pointer',
    ].join();
    return '${severity.name} $code: $message${where.isEmpty ? '' : ' ($where)'}'
        '${suggestion != null ? ' — $suggestion' : ''}';
  }
}

/// True when any diagnostic in [list] is an error.
bool hasErrors(Iterable<Diagnostic> list) => list.any((d) => d.isError);
