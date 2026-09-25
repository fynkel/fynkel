#!/usr/bin/env python3
"""The local release store (ADR-0029 §4, Order step 6).

Everything a Fynkel release publishes, put into one directory that a static
HTTP server can serve as a pub server and a release host at once:

    <store>/
      api/packages/<name>                  the pub listing (repository spec v2)
      packages/<name>/versions/<v>.tar.gz  the package archives
      runtime/<version>/<file>             the prebuilt runtime, one per target
      cli/<version>/fynkel-<target>        the `fynkel` CLI, built knowing no checkout
      index.json                           what is in the store, with checksums

The Dart packages are fynkel, fynkel_flutter, fynkel_generator and
fynkel_editor_api at the workspace version; their path dependencies on each
other become hosted ones on the store, and package:fynkel's fynkel.lock.json
names the store's runtime directory with each library's SHA-256.

Nothing here publishes anything: the store is a directory on this machine,
and the real publish waits for ADR-0005 and the user's word.

    python3 tools/release_store.py build [--store DIR] [--url URL] [--targets macos-arm64,macos-x64]
    python3 tools/release_store.py serve [--store DIR] [--port N]
    python3 tools/release_store.py rehearse [--store DIR] [--port N] [--work DIR] [--no-build-app]

`rehearse` builds the store, serves it, and then, outside the checkout and
with no cargo on PATH, has the store's CLI create a game with hosted
dependencies and runs `flutter pub get`, `fynkel validate`, `fynkel test
--headless`, `flutter test` (the hook must take the prebuilt runtime) and
`flutter build macos --debug`.
"""

import argparse
import functools
import hashlib
import http.server
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import threading
from pathlib import Path

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("release_store.py needs PyYAML (pip3 install pyyaml) to list the pubspecs")

ROOT = Path(__file__).resolve().parent.parent
PACKAGES = ["fynkel", "fynkel_flutter", "fynkel_generator", "fynkel_editor_api"]
CRATE = "fynkel_platform_apple"
MODULES = "physics,audio,animation"
# The hook's target keys (packages/fynkel/lib/src/hook/resolve.dart) → Rust.
TRIPLES = {
    "macos-arm64": "aarch64-apple-darwin",
    "macos-x64": "x86_64-apple-darwin",
    "ios-arm64": "aarch64-apple-ios",
    "ios-simulator-arm64": "aarch64-apple-ios-sim",
    "ios-simulator-x64": "x86_64-apple-ios",
}
DEFAULT_STORE = Path("/tmp/fynkel-store")
DEFAULT_PORT = 8799


def log(msg):
    print(f"[store] {msg}", flush=True)


def run(cmd, cwd=None, env=None, capture=False):
    log(f"$ {' '.join(str(c) for c in cmd)}" + (f"   (in {cwd})" if cwd else ""))
    result = subprocess.run(
        [str(c) for c in cmd], cwd=cwd, env=env, text=True,
        stdout=subprocess.PIPE if capture else None, stderr=subprocess.STDOUT if capture else None,
    )
    if result.returncode != 0:
        if capture:
            print(result.stdout)
        sys.exit(f"[store] failed ({result.returncode}): {' '.join(str(c) for c in cmd)}")
    return result.stdout if capture else ""


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def workspace_version():
    text = (ROOT / "Cargo.toml").read_text()
    m = re.search(r'\[workspace\.package\][^\[]*?\nversion\s*=\s*"([^"]+)"', text)
    if not m:
        sys.exit("[store] no [workspace.package] version in Cargo.toml")
    return m.group(1)


def host_triple():
    out = run(["rustc", "-vV"], capture=True)
    return re.search(r"^host: (\S+)", out, re.M).group(1)


# ---- runtime and CLI ----------------------------------------------------------


def build_runtime(target, host):
    """The game runtime exactly as the hook builds it: every module, no
    optional feature (`cargoBuildArgs`)."""
    triple = TRIPLES[target]
    cmd = ["cargo", "build", "--release", "-p", CRATE, "--no-default-features", "--features", MODULES]
    if triple != host:
        cmd += ["--target", triple]
    run(cmd, cwd=ROOT)
    sub = "" if triple == host else f"{triple}/"
    return ROOT / f"target/{sub}release/lib{CRATE}.dylib"


def build_cli():
    """The CLI as a release ships it: `FYNKEL_RELEASE` makes it forget the
    checkout it was built from (apps/cli/src/schemas.rs `repo_root`)."""
    env = dict(os.environ, FYNKEL_RELEASE="1")
    run(["cargo", "build", "--release", "-p", "fynkel_cli"], cwd=ROOT, env=env)
    return ROOT / "target/release/fynkel"


# ---- Dart packages ------------------------------------------------------------


def package_files(name):
    """The package's tracked files (what a `git clone` has), minus the
    generated FFI test stubs nobody else needs."""
    out = run(["git", "ls-files", "-z", f"packages/{name}"], cwd=ROOT, capture=True)
    prefix = f"packages/{name}/"
    for rel in sorted(p for p in out.split("\0") if p):
        path = ROOT / rel
        if path.is_file():
            yield rel[len(prefix):], path


def hosted_pubspec(text, url, version):
    """`publish_to: none` goes; `<dep>:\\n    path: ../<pkg>` becomes hosted."""
    text = re.sub(r"^publish_to:.*\n", "", text, flags=re.M)

    def hosted(m):
        return f"{m.group(1)}{m.group(2)}:\n{m.group(1)}  hosted: {url}\n{m.group(1)}  version: \"^{version}\"\n"

    return re.sub(r"^( +)(\w+):\n +path: \.\./(\w+)\n", hosted, text, flags=re.M)


def package_lock(text, url, version, runtime):
    """package:fynkel's lock naming the store's runtime directory, with the
    checksum of every library the store has (the others stay null)."""
    lock = json.loads(text)
    lock["runtime"]["store"] = f"{url}/runtime/{{version}}/"
    for target, entry in lock["runtime"]["artifacts"].items():
        entry["sha256"] = runtime.get(target, {}).get("sha256")
    if lock["runtime"]["version"] != version:
        sys.exit(f"[store] packages/fynkel/fynkel.lock.json pins runtime {lock['runtime']['version']}, "
                 f"the workspace is {version}")
    return json.dumps(lock, indent=2, ensure_ascii=False) + "\n"


def archive(name, url, version, runtime):
    buf = io.BytesIO()
    pubspec = None
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.PAX_FORMAT) as tar:
        for rel, path in package_files(name):
            data = path.read_bytes()
            if rel == "pubspec.yaml":
                data = hosted_pubspec(data.decode(), url, version).encode()
                pubspec = yaml.safe_load(data)
            elif name == "fynkel" and rel == "fynkel.lock.json":
                data = package_lock(data.decode(), url, version, runtime).encode()
            info = tarfile.TarInfo(rel)
            info.size = len(data)
            info.mode = 0o755 if os.access(path, os.X_OK) else 0o644
            info.mtime = 0
            tar.addfile(info, io.BytesIO(data))
    if pubspec is None or pubspec.get("version") != version:
        sys.exit(f"[store] packages/{name}/pubspec.yaml is not at version {version}")
    return buf.getvalue(), pubspec


# ---- the store ----------------------------------------------------------------


def build(store, url, targets):
    url = url.rstrip("/")
    version = workspace_version()
    host = host_triple()
    if store.exists():
        shutil.rmtree(store)
    log(f"Fynkel {version} into {store}, served as {url}")

    runtime = {}
    for target in targets:
        lib = build_runtime(target, host)
        dest = store / "runtime" / version / f"lib{CRATE}-{target}.dylib"
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(lib, dest)
        runtime[target] = {"file": dest.name, "sha256": sha256(dest), "bytes": dest.stat().st_size}
        log(f"runtime {target}: {dest.stat().st_size:,} bytes")

    cli = build_cli()
    host_key = next(k for k, v in TRIPLES.items() if v == host)
    cli_dest = store / "cli" / version / f"fynkel-{host_key}"
    cli_dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(cli, cli_dest)

    packages = {}
    for name in PACKAGES:
        data, pubspec = archive(name, url, version, runtime)
        dest = store / "packages" / name / "versions" / f"{version}.tar.gz"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        digest = hashlib.sha256(data).hexdigest()
        entry = {
            "version": version,
            "archive_url": f"{url}/packages/{name}/versions/{version}.tar.gz",
            "archive_sha256": digest,
            "pubspec": pubspec,
        }
        listing = store / "api" / "packages" / name
        listing.parent.mkdir(parents=True, exist_ok=True)
        listing.write_text(json.dumps({"name": name, "latest": entry, "versions": [entry]}, indent=2))
        packages[name] = {"archive": str(dest.relative_to(store)), "sha256": digest, "bytes": len(data)}
        log(f"package {name} {version}: {len(data):,} bytes")

    index = {
        "version": version,
        "url": url,
        "runtime": runtime,
        "cli": {host_key: {"file": str(cli_dest.relative_to(store)), "sha256": sha256(cli_dest)}},
        "packages": packages,
    }
    (store / "index.json").write_text(json.dumps(index, indent=2) + "\n")
    log(f"done: {store}/index.json")
    return index


class _Handler(http.server.SimpleHTTPRequestHandler):
    """A static server whose pub listings (`api/packages/<name>`, no
    extension) say they are pub's JSON."""

    def guess_type(self, path):
        if "/api/packages/" in str(path).replace(os.sep, "/"):
            return "application/vnd.pub.v2+json"
        return super().guess_type(path)

    def log_message(self, fmt, *args):
        pass


def serve(store, port):
    handler = functools.partial(_Handler, directory=str(store))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    log(f"serving {store} at http://127.0.0.1:{port}")
    return server


# ---- the rehearsal ------------------------------------------------------------


def which(tool):
    found = shutil.which(tool)
    if not found:
        sys.exit(f"[store] `{tool}` is not on PATH")
    return Path(found).resolve().parent


def rehearse(store, port, work, build_app):
    url = f"http://127.0.0.1:{port}"
    index = build(store, url, ["macos-arm64", "macos-x64"])
    server = serve(store, port)
    try:
        if work.exists():
            shutil.rmtree(work)
        (work / "bin").mkdir(parents=True)
        host_key = next(iter(index["cli"]))
        cli = work / "bin" / "fynkel"
        shutil.copy2(store / index["cli"][host_key]["file"], cli)
        # No cargo, no checkout: PATH has the store's CLI, Flutter and the
        # system; pub gets its own cache, so nothing comes from an earlier run.
        env = {
            k: v for k, v in os.environ.items()
            if k not in ("FYNKEL_CLI", "PUB_HOSTED_URL", "CARGO_HOME", "RUSTUP_HOME")
        }
        env["PATH"] = os.pathsep.join([str(work / "bin"), str(which("flutter")), "/usr/bin", "/bin", "/usr/sbin", "/sbin"])
        env["PUB_CACHE"] = str(work / "pub-cache")
        if shutil.which("cargo", path=env["PATH"]):
            sys.exit("[store] cargo is still on the rehearsal's PATH")

        game = work / "my_game"
        run([cli, "create", "my_game", "--store", url], cwd=work, env=env)
        pubspec = (game / "pubspec.yaml").read_text()
        if "path:" in pubspec or f"hosted: {url}" not in pubspec:
            sys.exit("[store] the created pubspec does not depend on the store")
        run(["flutter", "pub", "get"], cwd=game, env=env)
        config = json.loads((game / ".dart_tool/package_config.json").read_text())
        for p in config["packages"]:
            if p["name"] in PACKAGES and str(ROOT) in p["rootUri"]:
                sys.exit(f"[store] {p['name']} resolved into the checkout: {p['rootUri']}")
        run([cli, "validate"], cwd=game, env=env)
        run([cli, "test", "--headless", "--steps", "120"], cwd=game, env=env)
        out = run(["flutter", "test"], cwd=game, env=env, capture=True)
        print(out)
        if "from prebuilt" not in out:
            sys.exit("[store] the hook did not take the prebuilt runtime")
        if build_app:
            out = run(["flutter", "build", "macos", "--debug"], cwd=game, env=env, capture=True)
            print(out[-2000:])
        log(f"rehearsal passed: {game} built against {url} with no checkout and no cargo")
    finally:
        server.shutdown()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--store", type=Path, default=DEFAULT_STORE)
    b.add_argument("--url", default=f"http://127.0.0.1:{DEFAULT_PORT}")
    b.add_argument("--targets", default="macos-arm64,macos-x64")
    s = sub.add_parser("serve")
    s.add_argument("--store", type=Path, default=DEFAULT_STORE)
    s.add_argument("--port", type=int, default=DEFAULT_PORT)
    r = sub.add_parser("rehearse")
    r.add_argument("--store", type=Path, default=DEFAULT_STORE)
    r.add_argument("--port", type=int, default=DEFAULT_PORT)
    r.add_argument("--work", type=Path, default=Path("/tmp/fynkel-rehearsal"))
    r.add_argument("--no-build-app", action="store_true")
    args = parser.parse_args()
    if args.cmd == "build":
        targets = [t for t in args.targets.split(",") if t]
        unknown = [t for t in targets if t not in TRIPLES]
        if unknown:
            sys.exit(f"[store] unknown targets {unknown} (known: {', '.join(TRIPLES)})")
        build(args.store, args.url, targets)
    elif args.cmd == "serve":
        server = serve(args.store, args.port)
        try:
            threading.Event().wait()
        except KeyboardInterrupt:
            server.shutdown()
    else:
        rehearse(args.store, args.port, args.work, not args.no_build_app)


if __name__ == "__main__":
    main()
