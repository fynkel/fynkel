# M0 backlog #9 — Flutter overlay + pointer/keyboard focus test (Path A)

With Path A the 3D viewport is an ordinary Flutter layer (`Texture`), so
input never has to be routed between a native view and Flutter. This test
checks that the normal Flutter widgets around it behave the way a game HUD
needs. Driven with `cliclick` (real CGEvents; `System Events` `click at` does
**not** reach Flutter) after positioning the window with `osascript`.

Harness: `Focus` + `Listener(HitTestBehavior.opaque)` around the `Texture`
(what the engine would receive: pointer in viewport-physical pixels, key
down/up set); HUD = stats text, `TextField`, `FilledButton`, spinner.

| Step | Expected | Observed |
| --- | --- | --- |
| App starts, window activated | viewport owns keyboard focus | `viewport focus=true` at 0.04 s; `q` → `key down Q (viewport)` |
| Click empty viewport area | pointer down in physical px, focus stays on viewport | `pointer down at 400,1136`; `w a d` delivered to viewport |
| Click HUD `TextField`, type `hi w` | text field takes focus; no keys reach the engine | `hud text focus=true`; 0 key events to viewport |
| Click viewport again, type `s` | focus returns to viewport | `viewport focus=true`, `key down S (viewport)` |
| Click HUD button drawn over the 3D world | button fires, not the viewport | `HUD button click #1`; viewport clicks unchanged |
| Hover across viewport | hover positions in physical px | `pointer=800,936 (…HoverEvent)` |

Two things worth remembering for the real `FynkelViewport` widget:

1. **Focus race with `TextField`.** A focused `TextField` unfocuses on tap
   outside *during the same pointer dispatch*; a synchronous
   `requestFocus()` from the viewport's pointer-down handler loses that race
   and the next key is dropped. Defer the request (`Future.microtask`) —
   fixed and verified above.
2. **Key handling policy.** The viewport returns `handled` only for
   single-character game keys and `ignored` otherwise, so ⌘Q, menu
   shortcuts and Tab traversal keep working.
3. Overlay text is hit-testable: a stats box over the world swallows clicks,
   as any HUD widget would. `IgnorePointer` is the tool when a HUD element is
   display-only.

Verdict: overlay + input/focus gate passes with no native-view interop needed.
