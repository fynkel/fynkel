// The libraries a released package downloads: lib/src/prebuilt.dart, from a
// server of the test's own, and the build hook itself in a package that has
// no repository around it.

import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:fynkel/hooks.dart';
import 'package:fynkel/src/prebuilt.dart';
import 'package:hooks/hooks.dart';
import 'package:test/test.dart';

const _triple = 'aarch64-linux-android';
final _library = List.generate(100000, (i) => i * 7 % 251);

void main() {
  late HttpServer server;
  late Directory dir;
  late List<String> asked;
  late void Function(HttpResponse response) answer;

  setUp(() async {
    dir = Directory.systemTemp.createTempSync('fynkel_prebuilt');
    addTearDown(() => dir.deleteSync(recursive: true));
    asked = [];
    answer = (response) => response.add(_library);
    server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    server.listen((request) async {
      asked.add(request.uri.path);
      try {
        answer(request.response);
        await request.response.close();
      } on Object {
        // A short answer, on purpose.
      }
    });
  });

  Prebuilt prebuilt({String? url}) => Prebuilt(
    crate: 'fynkel_ffi',
    url: url ?? 'http://127.0.0.1:${server.port}/v1',
    files: {_triple: (sha256: '${sha256.convert(_library)}', bytes: _library.length)},
  );
  Future<File> fetch(Prebuilt p, {String triple = _triple, String? mirror}) => p.fetch(
    triple,
    cache: dir,
    mirror: mirror,
    stall: const Duration(seconds: 2),
    retryAfter: const Duration(milliseconds: 1),
  );
  Matcher failsWith(Object message) => throwsA(isA<HookException>().having((e) => e.message, 'message', message));
  List<File> left() => dir.listSync(recursive: true).whereType<File>().toList();

  test('prebuilt.json reads back as it was written', () {
    final p = Prebuilt.parse(prebuilt().toJson(), 'prebuilt.json');
    expect((p.crate, p.url), (prebuilt().crate, prebuilt().url));
    expect(p.files, prebuilt().files);
    expect(() => Prebuilt.parse('{"crate": 3}', 'a/prebuilt.json'), failsWith(contains('a/prebuilt.json is damaged')));
  });

  test('a library downloads once', () async {
    final p = prebuilt();
    final file = await fetch(p);
    expect(file.readAsBytesSync(), _library);
    await server.close(force: true);
    expect((await fetch(p)).path, file.path, reason: 'no network the second time');
    expect(asked, ['/v1/fynkel_ffi-aarch64-linux-android.so']);
    expect(left(), hasLength(1));
  });

  test('one damaged on disk downloads again', () async {
    (await fetch(prebuilt())).writeAsStringSync('damaged');
    expect((await fetch(prebuilt())).readAsBytesSync(), _library);
    expect(asked, hasLength(2));
  });

  test("a mirror takes the release's place", () async {
    await fetch(prebuilt(url: 'http://127.0.0.1:1/'), mirror: 'http://127.0.0.1:${server.port}/mine/');
    expect(asked, ['/mine/fynkel_ffi-aarch64-linux-android.so']);
  });

  test('another file than the released one is not used', () async {
    answer = (response) => response.add([..._library.skip(1), 0]);
    await expectLater(fetch(prebuilt()), failsWith(contains('not the file this package was released with')));
    expect(left(), isEmpty);
  });

  test('a longer or a shorter one is not either', () async {
    answer = (response) => response.add([..._library, 0]);
    await expectLater(fetch(prebuilt()), failsWith(contains('longer than')));
    answer = (response) => response.add(_library.sublist(5));
    await expectLater(fetch(prebuilt()), failsWith(contains('it stopped after')));
    expect(left(), isEmpty);
  });

  test('a server that is busy for a moment is asked again, one that has no such file is not', () async {
    answer = (response) => asked.length < 3 ? response.statusCode = 503 : response.add(_library);
    expect((await fetch(prebuilt())).readAsBytesSync(), _library);
    asked.clear();
    (await fetch(prebuilt())).deleteSync();
    answer = (response) => response.statusCode = 404;
    await expectLater(fetch(prebuilt()), failsWith(contains('the server answered 404')));
    expect(asked, hasLength(1));
  });

  test('without a network it says what to do', () async {
    await expectLater(fetch(prebuilt(url: 'http://127.0.0.1:1/')), failsWith(contains('check the connection')));
  });

  test('a target the release lacks is named', () async {
    await expectLater(
      fetch(prebuilt(), triple: 'x86_64-apple-ios'),
      failsWith(contains('no fynkel_ffi for x86_64-apple-ios')),
    );
  });

  group('the build hook, with no repository around the package,', () {
    Future<void> hook(void Function(BuildInput input, BuildOutput output) check) => testCodeBuildHook(
      mainMethod: (args) =>
          build(args, (input, output) => buildRustCrate(input, output, crate: 'fynkel_ffi', asset: 'fynkel')),
      targetOS: OS.android,
      targetArchitecture: Architecture.arm64,
      targetAndroidNdkApi: 30,
      check: check,
    );

    // testCodeBuildHook runs in the folder it is called from, as the package.
    void inPackage(String? manifest) {
      final package = Directory('${dir.path}/hosted/pub.dev/fynkel-0.1.0')..createSync(recursive: true);
      File('${package.path}/pubspec.yaml').writeAsStringSync('name: fynkel\n');
      if (manifest != null) File('${package.path}/prebuilt.json').writeAsStringSync(manifest);
      final before = Directory.current;
      Directory.current = package;
      addTearDown(() => Directory.current = before);
    }

    test('hands over the downloaded library', () async {
      inPackage(prebuilt().toJson());
      await hook((input, output) {
        final asset = output.assets.code.single;
        expect(asset.id, 'package:fynkel/fynkel');
        expect(File.fromUri(asset.file!).readAsBytesSync(), _library);
        expect(asset.file!.pathSegments.last, 'libfynkel_ffi.so');
        expect(output.dependencies, [input.packageRoot.resolve('prebuilt.json')]);
      });
    });

    test('says what is missing without a prebuilt.json', () async {
      inPackage(null);
      await expectLater(hook((_, _) {}), failsWith(contains('a release on pub.dev has')));
    });
  });
}
