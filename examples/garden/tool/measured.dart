// The garden for package:fynkel_measure's tool, which runs this in place
// of lib/main.dart. The bench adds the garden's own tap: a cast from the
// camera to the character's middle.

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_measure/fynkel_measure.dart';
import 'package:garden/garden.dart';

void main() => runApp(MeasuredApp(Garden.new, timings: _tap));

List<String> _tap(GameObject garden) {
  final from = garden.world.first<OrbitCamera>()!.transform.position;
  return ['raycast, the garden, at the character: ${castTimes(garden.world, Ray(from, OrbitCamera.target - from))}'];
}
