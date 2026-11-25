---
title: "Before you release"
description: "Name, icon, version, and what to check on a real device."
---

A Fynkel game ships as any Flutter app does. These pages go through it; the
stores' own rules are Flutter's
([iOS](https://docs.flutter.dev/deployment/ios),
[Android](https://docs.flutter.dev/deployment/android)).

## A name nobody else has

`fynkel create my_game --org com.yourname` makes the app `com.yourname.my_game`
on both stores' systems. It can't change once it's published, and the default,
`com.example`, is refused. In a game made already, change it in:

- `ios/Runner.xcworkspace`, opened in Xcode: Runner, Signing & Capabilities,
  Bundle Identifier
- `android/app/build.gradle.kts`: `applicationId`

The name under the icon is `CFBundleDisplayName` in `ios/Runner/Info.plist`
and `android:label` in `android/app/src/main/AndroidManifest.xml`.

## An icon

From one square picture, 1024 × 1024, with no see-through parts:

```sh
flutter pub add dev:flutter_launcher_icons
```

```yaml
# pubspec.yaml
flutter_launcher_icons:
  image_path: icon.png
  ios: true
  android: true
  remove_alpha_ios: true
```

```sh
dart run flutter_launcher_icons
```

## The version

`version: 1.0.0+1` in `pubspec.yaml`: players see what's before the `+`. The
number after it must be higher in every build you upload.

## Check it on a device

A release build isn't a debug build made faster: errors aren't shown over
the game, and timing differs.

```sh
flutter run --release -d <device>
```

- Play it from a fresh install to the end.
- Send it to the background in the middle, and come back. Take a call.
- Turn the phone, if it can turn.
- Play ten minutes on the oldest phone you support, and see that it keeps
  its speed as it warms up ([Measuring](/making/measuring/)).
- Fly in aeroplane mode: a game that doesn't need the network should start.

Errors in a release build go to `world.errors`: send them to a crash
reporter, or they're lost ([Errors](/basics/errors/)).

## The licences

Flutter shows the licences of the packages an app uses, Fynkel's too, on a
page made for it: `showLicensePage(context: context)`. Give it a button in
your settings.
