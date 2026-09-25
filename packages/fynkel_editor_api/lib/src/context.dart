/// The editor, as an extension's callbacks see it (ADR-0028 §4). Every
/// callback is given one; it speaks for the package that registered the
/// callback.
abstract interface class EditorContext {
  /// The package it speaks for.
  String get package;

  /// A line in the Console, attributed to [package] (Unity's `Debug.Log`).
  void log(String message);

  /// A warning in the Console (`Debug.LogWarning`).
  void logWarning(String message);

  /// An error in the Console (`Debug.LogError`).
  void logError(String message);
}
