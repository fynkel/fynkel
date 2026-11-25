---
title: "App Store"
description: "Sign, build and upload for iPhone and iPad."
---

You need to be in the [Apple Developer
Program](https://developer.apple.com/programs/), and to have done [Before
you release](/release/before/).

## 1. Sign it

Open `ios/Runner.xcworkspace` in Xcode. Under Runner, Signing &
Capabilities, tick "Automatically manage signing" and choose your team.

## 2. Say that it uses no encryption of its own

In `ios/Runner/Info.plist`. Without it App Store Connect asks at every
upload. (Say what's true of your game: HTTPS alone is exempt.)

```xml
<key>ITSAppUsesNonExemptEncryption</key>
<false/>
```

## 3. Frame rate and turning

For 120 frames a second on the iPhones that have it, `Info.plist` needs
`CADisableMinimumFrameDurationOnPhone` set to `true`; a new app has it.
Remove the orientations your game doesn't have from
`UISupportedInterfaceOrientations` and its `~ipad` twin.

An iPad app that doesn't have all four needs `UIRequiresFullScreen` set to
`true`.

## 4. Build

```sh
flutter build ipa
```

It makes `build/ios/ipa/<name>.ipa`. Fynkel's libraries are in it as signed
frameworks; there's nothing to add.

## 5. Upload

Make the app's page in [App Store
Connect](https://appstoreconnect.apple.com) (My Apps, +). Then drop the
`.ipa` into Apple's
[Transporter](https://apps.apple.com/app/transporter/id1450874784), sign in
and press Deliver.

After some minutes the build is in App Store Connect, under TestFlight. Give
it to a few people there first: up to 100 of your own team get it at once,
and others after a short review.

## 6. The page

- **Screenshots** of the sizes App Store Connect asks for. Take them on a
  simulator, or draw them from the game itself: a `HeadlessView` draws any
  size ([Testing a game](/making/testing/#checking-the-whole-picture)).
- **App privacy.** Fynkel collects nothing and sends nothing. Answer for your
  game and for the other packages it uses.
- **Age rating**, a description, a support link.

Then choose the build, and send it for review.
