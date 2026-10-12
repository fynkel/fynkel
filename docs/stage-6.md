# Stage 6 plan: the review

Status: a draft for you to review (2026-10-12). Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 5 is done
([report](stage-5-report.md)).

Five stages in, Fynkel has a core (`fynkel`, 8,600 lines of Dart), the
Flutter side (`fynkel_flutter`, 900), physics (`fynkel_physics`, 3,700),
an engine in Rust (26,000 lines in nine crates), five games (83 to 793
lines), a lab of 28 scenes, and a guide. Each stage checked its own
features. No stage has stepped back to ask of the whole:

- **Is it good?** Does it do what a game needs, the way a developer
  expects, without surprises?
- **Is it understandable?** Can someone who has never seen it read the
  guide and write a real game, simply, without fighting it?
- **Does it scale?** To a game of 50 files and many kinds of object, to
  a team, to new packages (audio, animation) that plug in as physics did,
  and to long sessions on a phone?

Stage 6 answers these with evidence, not opinion, and fixes what it
finds. It adds no new features, except where a fix needs one.

Stage 7 is planned to be **Flutter and the game**: menus, pop-ups and
dialogs over a game, pausing under them, themes passed from the app into
the game (a dark theme switched in the app, and the game follows), game
state shown in widgets, navigation with a game behind it, the app going
to the background, several views. Stage 6 notes what it finds there, and
stage 7 builds on it.

## 1. How we find out

**Fresh eyes.** The best test of "can people write real code with it
simply" is people who don't know it. So: several new agents, each given
only the guide and the packages' API docs (no repository, no history),
each write a small game from a one-paragraph brief. Everything they do is
kept: where they guessed wrong, errors they hit and whether the message
told them why, what they looked for and didn't find, where they worked
around the framework. Briefs that cover different ground:

1. A title screen, a level, a pause menu and a game-over pop-up (Flutter
   and the game together).
2. A top-down shooter: many enemies, spawning and pooling, signals, a
   score.
3. A physics toy: stacking, dragging bodies, joints, triggers.
4. A character walking a small level with a following camera and
   pickups.

**A bigger game**, written by me the same way: a game of 20 or more files
and several kinds of object, to see how a codebase grows. Where does code
go, what gets shared, what gets awkward?

**An API review**, of every public name in the three packages, against
each other and against Unity: one name for one thing, the same words
across packages, good defaults, a doc comment on everything, nothing
that can be done two ways without a reason, and errors that say what to
do.

**An architecture review**, of the code itself:

- Files that are too large (`motion.dart` 1,126 lines, `input.dart`
  1,033, `game_object.dart` 930).
- The boundaries between the core, the Flutter side, physics and the
  engine.
- `extend.dart` as the way in for packages: could an audio package plug
  in without changing the core? We sketch one to find out (sketched only,
  not built).
- The C interfaces and their versions.

**Scale and stability checks**:

- A world with 10,000 objects of 50 kinds.
- 30 minutes of play on each phone, with memory read at intervals.
- Errors in every lifecycle method, to show the game goes on.
- Hot reload in the middle of play, with physics running.

## 2. Steps

1. **The briefs.** Four fresh agents write their games; each keeps a log
   of every stumble. *Done when* the four games run (or the log says why
   they don't), and the findings are in one list.
2. **The API review.** Every public name, in a table: kept, renamed,
   merged, removed or documented. *Done when* the table is written and
   you have agreed it.
3. **The architecture review**, with the audio-package sketch. *Done
   when* the findings are written down, each with a fix or a reason to
   leave it.
4. **The bigger game** (20+ files). *Done when* it runs on the three
   devices and its log is in the list.
5. **Scale and stability.** *Done when* each check above has its numbers.
6. **The fixes**, in order of how much they hurt, a PR each, with the
   guide changed to match. Breaking changes are allowed (nothing is
   published yet), each with a line in the guide's notes for anyone who
   wrote code before. *Done when* every finding is fixed or left with a
   reason you agree with.
7. **Again, with fresh eyes**: two new agents write briefs 1 and 2 again
   on the fixed framework. *Done when* their logs are shorter, and the
   stumbles left are understood.
8. **The report**, with the findings before and after, and what goes to
   stage 7. *Done when* you have read it.

## 3. Questions

Each with a recommended answer.

1. **Fresh agents as the test developers?** Recommended: yes, four in
   step 1 and two in step 7. Each run costs tokens: about one large task
   each. Or I write them all myself, which is cheaper, but I know the
   framework too well to stumble where a newcomer would.
2. **Will you write something yourself too?** Recommended: yes, a small
   game of your choice in step 4's time, as the developer the framework
   is for. Your log weighs more than any agent's.
3. **Breaking changes?** Recommended: allowed in this stage, before
   anyone else depends on the API; after stage 6 the API changes only
   with a reason written down.
4. **The engine (Rust) in scope?** Recommended: its boundary (the C
   interfaces, errors coming back, versions) and anything a scale check
   points at; not a rewrite of its insides.
5. **The lab and the examples?** Recommended: they're code people will
   copy, so they're reviewed as API users too, and fixed with the rest.
