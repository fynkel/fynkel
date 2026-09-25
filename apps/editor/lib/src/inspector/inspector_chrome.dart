// The pieces every Inspector page is built from (item 9b/9c), so an
// entity, an asset, a part and a folder read the same way: the page header
// (picture, name, what it is, the padlock), the facts row, uppercase
// section captions, label/value rows on the fields' label column, and the
// apply bar pinned to the bottom while something is not applied yet.
import 'package:flutter/material.dart';

import '../shell/editor_selection.dart';
import '../theme/icons.dart';
import '../theme/lock_button.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart' show labelColumnWidth;

/// An uppercase section caption: `IMPORT SETTINGS`, `PARTS · 4`.
class InspectorCaption extends StatelessWidget {
  const InspectorCaption(this.text, {super.key, this.trailing});

  final String text;
  final Widget? trailing;

  // Unity's [Header]: the group's name in bold, as written — not a
  // spaced-out capital label (docs/ui-refresh).
  static TextStyle style(BuildContext context) => TextStyle(
    fontFamily: uiFontFamily,
    fontSize: 11,
    fontWeight: FontWeight.w600,
    letterSpacing: -0.15,
    color: context.tokens.text,
  );

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: Text(text, style: style(context), maxLines: 1, overflow: TextOverflow.ellipsis),
        ),
        ?trailing,
      ],
    );
  }
}

/// A page section: its caption, then its rows, on the page's padding.
class InspectorSection extends StatelessWidget {
  const InspectorSection({super.key, required this.title, required this.children, this.trailing});

  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Padding(
      padding: EdgeInsets.fromLTRB(t.pad + 2, 12, t.pad + 2, 0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: InspectorCaption(title, trailing: trailing),
          ),
          ...children,
        ],
      ),
    );
  }
}

/// A label and a read-only value on the fields' label column.
class InspectorValueRow extends StatelessWidget {
  const InspectorValueRow(this.label, this.value, {super.key, this.mono = false, this.trailing});

  final String label;
  final String value;
  final bool mono;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return LayoutBuilder(
      builder: (context, c) => ConstrainedBox(
        constraints: BoxConstraints(minHeight: t.row + 4),
        child: Row(
          children: [
            SizedBox(
              width: labelColumnWidth(c.maxWidth),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: SelectableText(
                value,
                maxLines: 3,
                style: mono ? monoStyle(context, color: t.text) : theme.textTheme.bodySmall?.copyWith(color: t.text),
              ),
            ),
            ?trailing,
          ],
        ),
      ),
    );
  }
}

/// Three facts side by side under a page header: `KIND mesh`, `SIZE 2.1 MB`.
class FactsRow extends StatelessWidget {
  const FactsRow(this.facts, {super.key});

  final List<(String, String)> facts;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return Container(
      margin: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 0),
      decoration: BoxDecoration(
        border: Border.all(color: t.line),
        borderRadius: BorderRadius.circular(t.radiusPanel),
      ),
      child: IntrinsicHeight(
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final (i, (label, value)) in facts.indexed)
              Expanded(
                child: Container(
                  padding: const EdgeInsets.fromLTRB(8, 5, 6, 6),
                  decoration: BoxDecoration(border: i == 0 ? null : Border(left: t.hairline)),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(label.toUpperCase(), style: InspectorCaption.style(context)),
                      const SizedBox(height: 2),
                      Text(
                        value,
                        key: ValueKey('facts.$label'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.bodySmall?.copyWith(color: t.text),
                      ),
                    ],
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// The top of a page: a picture, the name and a line saying what it is,
/// then the page's own buttons and the padlock.
class InspectorPageHeader extends StatelessWidget {
  const InspectorPageHeader({
    super.key,
    required this.leading,
    required this.title,
    this.titleKey,
    this.subtitle,
    this.selection,
    this.below,
  });

  final Widget leading;
  final Widget title;
  final Key? titleKey;
  final Widget? subtitle;
  final EditorSelection? selection;

  /// Under the name: an id row, a back-link.
  final Widget? below;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Container(
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: EdgeInsets.fromLTRB(t.pad + 2, 8, 4, 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          leading,
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                DefaultTextStyle.merge(
                  style: Theme.of(context).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600, color: t.text),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  child: KeyedSubtree(key: titleKey, child: title),
                ),
                if (subtitle != null)
                  DefaultTextStyle.merge(
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textSecondary),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    child: subtitle!,
                  ),
                ?below,
              ],
            ),
          ),
          if (selection != null) InspectorLockButton(selection: selection!),
        ],
      ),
    );
  }
}

/// The padlock: pins what the Inspector shows while the selection moves on.
class InspectorLockButton extends StatelessWidget {
  const InspectorLockButton({super.key, required this.selection});

  final EditorSelection selection;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: selection,
      builder: (context, _) => LockButton(
        key: const ValueKey('inspector.lock'),
        locked: selection.locked,
        lockTip: 'Lock: keep showing this while you select others',
        unlockTip: 'Unlock: follow the selection again',
        onChanged: selection.target == null ? null : selection.setLocked,
      ),
    );
  }
}

/// A small action button in a page's toolbar row (Open, Reveal, Reimport).
class PageAction extends StatelessWidget {
  const PageAction({super.key, required this.icon, required this.label, required this.onPressed, this.primary = false});

  final IconData icon;
  final String label;
  final VoidCallback? onPressed;
  final bool primary;

  @override
  Widget build(BuildContext context) {
    final style = ButtonStyle(
      visualDensity: VisualDensity.compact,
      padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 8)),
      minimumSize: WidgetStatePropertyAll(Size(0, context.tokens.row + 2)),
      iconSize: const WidgetStatePropertyAll(13),
    );
    return primary
        ? FilledButton.tonalIcon(style: style, onPressed: onPressed, icon: Icon(icon), label: Text(label))
        : OutlinedButton.icon(style: style, onPressed: onPressed, icon: Icon(icon), label: Text(label));
  }
}

/// Pinned to the bottom of a page while an edit waits to be applied:
/// what is pending, Revert, and the apply button.
class ApplyBar extends StatelessWidget {
  const ApplyBar({
    super.key,
    required this.message,
    required this.applyLabel,
    required this.onApply,
    required this.onRevert,
    this.applyKey,
    this.revertKey,
  });

  final String message;
  final String applyLabel;
  final VoidCallback? onApply;
  final VoidCallback? onRevert;
  final Key? applyKey;
  final Key? revertKey;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return Container(
      key: const ValueKey('inspector.applyBar'),
      decoration: BoxDecoration(
        color: t.surfaceOf(t.accent, .12),
        border: Border(top: BorderSide(color: t.accent.withValues(alpha: .5))),
      ),
      padding: EdgeInsets.fromLTRB(t.pad + 2, 6, t.pad, 6),
      // Narrow (a docked Inspector), the buttons go under the message.
      child: LayoutBuilder(
        builder: (context, c) {
          final note = Row(
            children: [
              Icon(FIcons.editNote, size: 14, color: t.accent),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  message,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.text),
                ),
              ),
            ],
          );
          final buttons = [
            TextButton(
              key: revertKey,
              style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
              onPressed: onRevert,
              child: const Text('Revert'),
            ),
            const SizedBox(width: 4),
            FilledButton(
              key: applyKey,
              style: FilledButton.styleFrom(visualDensity: VisualDensity.compact),
              onPressed: onApply,
              child: Text(applyLabel),
            ),
          ];
          if (c.maxWidth < 420) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                note,
                const SizedBox(height: 4),
                Wrap(alignment: WrapAlignment.end, runSpacing: 4, children: buttons),
              ],
            );
          }
          return Row(
            children: [
              Expanded(child: note),
              ...buttons,
            ],
          );
        },
      ),
    );
  }
}

/// A centred message for a page with nothing to show.
class InspectorEmpty extends StatelessWidget {
  const InspectorEmpty({super.key, required this.icon, required this.title, this.message});

  final IconData icon;
  final String title;
  final String? message;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 28, color: t.textDisabled),
            const SizedBox(height: 10),
            Text(title, style: theme.textTheme.bodyMedium?.copyWith(color: t.textSecondary)),
            if (message != null) ...[
              const SizedBox(height: 4),
              Text(
                message!,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
