import 'dart:io';

import 'package:fynkel_cli/fynkel_cli.dart';

void main(List<String> args) async {
  // Not exit(): what is still to be written is written first.
  exitCode = await FynkelCli().run(args);
}
