// hello for package:fynkel_measure's tool, which runs this in place of
// lib/main.dart.

import 'package:flutter/widgets.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

import 'main.dart';

void main() => runApp(const MeasuredApp(Hello.new));
