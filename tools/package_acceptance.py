#!/usr/bin/env python3
"""ADR-0029 step 7: code_only plays with the pickups package, both ways a
game can depend on one.

    python3 tools/package_acceptance.py [--work DIR] [--mode path|git|both]

For each way, a copy of examples/code_only and examples/packages/pickups
under WORK/<mode>/ (the originals are never touched):

- `path`: pickups next to the game, a path dependency (the user's own
  package: its assets edit in place);
- `git`: pickups committed to a local git repository and depended on by
  `file://` URL, so pub checks it out into its cache, as it would a package
  from pub.dev (read-only; Copy into Project). Its own dependencies on the
  Fynkel packages are hosted constraints, as a published package has them;
  the game overrides them with the checkout's.

Then, in the copy: `flutter pub get` (and where pickups resolved), `fynkel
validate`, `fynkel cook` (the package's assets cooked), `flutter test` (the
game plays: the gem is the package's prefab), and the project editor:
`fynkel editor --write-host` with tools/acceptance/packages_editor_test.dart
run inside the host, where pickups' real editor part is compiled in (its
prefab placed through its menu item, its Inspector page shown, the package
asset edited in place or read-only and copied into the project).

Needs flutter on PATH and the debug CLI (`cargo build -p fynkel_cli`, or
FYNKEL_CLI).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLI = Path(os.environ.get("FYNKEL_CLI", ROOT / "target/debug/fynkel"))
FYNKEL_PACKAGES = ["fynkel", "fynkel_flutter", "fynkel_editor_api", "fynkel_generator"]
VERSION = "0.0.1"


def log(msg):
    print(f"[acceptance] {msg}", flush=True)


def run(cmd, cwd, env=None, capture=False):
    log(f"$ {' '.join(str(c) for c in cmd)}   (in {cwd})")
    r = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, text=True,
                       stdout=subprocess.PIPE if capture else None,
                       stderr=subprocess.STDOUT if capture else None)
    if r.returncode != 0:
        if capture:
            print(r.stdout)
        sys.exit(f"[acceptance] failed ({r.returncode}): {' '.join(str(c) for c in cmd)}")
    return r.stdout if capture else ""


def copy_tracked(rel, dest):
    """A package's tracked files, as a clone has them."""
    out = subprocess.run(["git", "ls-files", "-z", rel], cwd=ROOT, text=True, capture_output=True, check=True).stdout
    for f in (p for p in out.split("\0") if p):
        src = ROOT / f
        if not src.is_file():
            continue
        target = dest / Path(f).relative_to(rel)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, target)


def absolute_fynkel_paths(text):
    """`path: ../../packages/<p>` (any depth) → the checkout's package."""
    return re.sub(r"path: (?:\.\./)+packages/(\w+)", lambda m: f"path: {ROOT / 'packages' / m.group(1)}", text)


def hosted_fynkel(text):
    """A package as published: its Fynkel dependencies are constraints."""
    return re.sub(r"^(  )(\w+):\n    path: (?:\.\./)+packages/\w+\n",
                  lambda m: f'{m.group(1)}{m.group(2)}: "^{VERSION}"\n', text, flags=re.M)


def prepare(mode, base):
    if base.exists():
        shutil.rmtree(base)
    game, pickups = base / "code_only", base / "pickups"
    copy_tracked("examples/code_only", game)
    copy_tracked("examples/packages/pickups", pickups)

    pubspec = pickups / "pubspec.yaml"
    text = pubspec.read_text()
    pubspec.write_text(absolute_fynkel_paths(text) if mode == "path" else hosted_fynkel(text))

    text = (game / "pubspec.yaml").read_text()
    if "    path: ../packages/pickups\n" not in text:
        sys.exit("[acceptance] code_only's pubspec no longer depends on ../packages/pickups")
    if mode == "path":
        text = text.replace("    path: ../packages/pickups\n", "    path: ../pickups\n")
    else:
        git = ["git", "-c", "user.name=Fynkel acceptance", "-c", "user.email=acceptance@fynkel.invalid"]
        run(["git", "init", "-q", "-b", "main"], pickups)
        run(git[:1] + ["add", "-A"], pickups)
        run(git + ["commit", "-q", "-m", "pickups for the ADR-0029 acceptance"], pickups)
        text = text.replace("    path: ../packages/pickups\n", f"    git:\n      url: {pickups.as_uri()}\n      ref: main\n")
        # pickups asks for the Fynkel packages by version; the game gives
        # them the checkout's (nothing is on pub.dev).
        overrides = "".join(f"  {p}:\n    path: {ROOT / 'packages' / p}\n" for p in FYNKEL_PACKAGES)
        text = text.rstrip("\n") + f"\n\ndependency_overrides:\n{overrides}"
    (game / "pubspec.yaml").write_text(absolute_fynkel_paths(text))
    return game, pickups


def check_resolution(mode, game, pickups):
    config = json.loads((game / ".dart_tool/package_config.json").read_text())
    uri = next(p["rootUri"] for p in config["packages"] if p["name"] == "pickups")
    # Relative URIs are relative to .dart_tool/.
    root = uri[len("file://"):] if uri.startswith("file://") else str((game / ".dart_tool" / uri).resolve())
    lock = (game / "pubspec.lock").read_text()
    source = re.search(r"^  pickups:\n(?:    .*\n)*?    source: (\w+)", lock, re.M).group(1)
    log(f"pickups resolved to {root} (pubspec.lock: source {source})")
    if mode == "path":
        if source != "path" or Path(root).resolve() != pickups.resolve():
            sys.exit(f"[acceptance] expected the path copy {pickups}, got {root}")
    else:
        if source != "git" or "/git/" not in root or str(pickups) in root:
            sys.exit(f"[acceptance] expected pub's git cache, got {root}")


def acceptance(mode, work, env):
    log(f"=== {mode}")
    game, pickups = prepare(mode, work / mode)
    run(["flutter", "pub", "get"], game, env)
    check_resolution(mode, game, pickups)
    run([CLI, "validate"], game, env)
    cooked = run([CLI, "cook", "--json"], game, env, capture=True)
    if "Packages/pickups/prefabs/pickup.prefab.json" not in cooked:
        sys.exit("[acceptance] the cook did not include the package's prefab")
    run(["flutter", "test", "test/game_test.dart"], game, env)

    host = Path(run([CLI, "editor", str(game), "--write-host"], game, env, capture=True).strip().splitlines()[-1])
    (host / "test").mkdir(exist_ok=True)
    shutil.copy2(ROOT / "tools/acceptance/packages_editor_test.dart", host / "test/packages_editor_test.dart")
    # The host is an app; a test needs flutter_test (this copy only).
    spec = host / "pubspec.yaml"
    if "flutter_test" not in spec.read_text():
        spec.write_text(spec.read_text() + "dev_dependencies:\n  flutter_test:\n    sdk: flutter\n")
    run(["flutter", "pub", "get"], host, env)
    editor_env = dict(env, FYNKEL_ACCEPTANCE_PROJECT=str(game), FYNKEL_ACCEPTANCE_MODE=mode)
    run(["flutter", "test", "test/packages_editor_test.dart"], host, editor_env)
    # What the editor wrote is a valid project still.
    run([CLI, "validate"], game, env)
    log(f"{mode}: passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--work", type=Path, default=Path("/tmp/fynkel-acceptance"))
    parser.add_argument("--mode", choices=["path", "git", "both"], default="both")
    args = parser.parse_args()
    if not CLI.is_file():
        sys.exit(f"[acceptance] no CLI at {CLI} (cargo build -p fynkel_cli, or set FYNKEL_CLI)")
    env = dict(os.environ, FYNKEL_CLI=str(CLI))
    for mode in (["path", "git"] if args.mode == "both" else [args.mode]):
        acceptance(mode, args.work.resolve(), env)
    log("ADR-0029 acceptance passed")


if __name__ == "__main__":
    main()
