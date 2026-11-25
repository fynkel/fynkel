#!/usr/bin/env python3
"""Stands in for pub.dev and the GitHub release on this machine, so that a
release can be installed and played before anything is public
(docs/releasing.md).

Serves the packages tool/release.dart made and the libraries
tool/prebuilt.dart built; asks pub.dev for every other package. Keep it
running, and in another terminal, with a cache of its own, so the rehearsed
packages stay out of the real one:

  export PUB_HOSTED_URL=http://127.0.0.1:8765 PUB_CACHE=/tmp/fynkel-rehearsal
  dart pub global activate fynkel_cli
  dart pub global run fynkel_cli:fynkel create my_game

python3 tools/rehearse.py [port]
"""

import hashlib
import http.server
import io
import json
import pathlib
import re
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGES = pathlib.Path(tempfile.gettempdir()) / 'fynkel-release-packages'
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
HERE = f'http://127.0.0.1:{PORT}'


def pack(folder: pathlib.Path) -> bytes:
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode='w:gz') as tar:
        for path in sorted(p for p in folder.rglob('*') if p.is_file()):
            content = path.read_bytes()
            if path.name == 'prebuilt.json':  # The libraries are here too.
                content = re.sub(rb'"url": "[^"]*"', f'"url": "{HERE}/libraries"'.encode(), content)
            info = tarfile.TarInfo(str(path.relative_to(folder)))
            info.size = len(content)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(content))
    return data.getvalue()


def main() -> None:
    packages = {}
    for folder in sorted(PACKAGES.iterdir()):
        pubspec = (folder / 'pubspec.yaml').read_text()
        spec = read_pubspec(pubspec)
        packages[folder.name] = (spec, pack(folder))
    version = packages['fynkel'][0]['version']
    libraries = ROOT / 'target' / 'prebuilt' / f'v{version}'

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            if m := re.fullmatch(r'/api/packages/(\w+)', self.path):
                if m[1] in packages:
                    spec, archive = packages[m[1]]
                    entry = {
                        'version': spec['version'],
                        'pubspec': spec,
                        'archive_url': f'{HERE}/archives/{m[1]}.tar.gz',
                        'archive_sha256': hashlib.sha256(archive).hexdigest(),
                    }
                    body = {'name': m[1], 'latest': entry, 'versions': [entry]}
                    return self.answer(json.dumps(body).encode(), 'application/vnd.pub.v2+json')
            if m := re.fullmatch(r'/archives/(\w+)\.tar\.gz', self.path):
                if m[1] in packages:
                    return self.answer(packages[m[1]][1], 'application/octet-stream')
            if m := re.fullmatch(r'/libraries/([\w.-]+)', self.path):
                path = libraries / m[1]
                if path.is_file():
                    return self.answer(path.read_bytes(), 'application/octet-stream')
                return self.send_error(404)
            try:
                headers = {'Accept': self.headers.get('Accept', '*/*')}
                request = urllib.request.Request(f'https://pub.dev{self.path}', headers=headers)
                with urllib.request.urlopen(request) as response:
                    self.answer(response.read(), response.headers.get('Content-Type', ''))
            except urllib.error.HTTPError as e:
                self.send_error(e.code)

        def answer(self, body, kind):
            self.send_response(200)
            self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    print(f'{", ".join(packages)} {version} at {HERE}', flush=True)
    http.server.ThreadingHTTPServer(('127.0.0.1', PORT), Handler).serve_forever()


def read_pubspec(text: str) -> dict:
    """As much of a pubspec of ours as pub resolves by."""
    spec: dict = {}
    section = None
    package = None
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        indent = len(line) - len(line.lstrip())
        key, _, value = line.strip().partition(': ')
        key, value = key.rstrip(':'), value.strip('"')
        if indent == 0:
            section = key if key in ('environment', 'dependencies') else None
            if section:
                spec[section] = {}
            elif key in ('name', 'version'):
                spec[key] = value
        elif section and indent == 2:
            package = key
            spec[section][key] = value or {}
        elif section and indent == 4 and package:
            spec[section][package] = {key: value}
    return spec


if __name__ == '__main__':
    main()
