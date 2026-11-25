---
title: "macOS"
description: "Sign, build and hand out a Mac game."
---

Do [Before you release](/release/before/) first. A Mac game goes out through
the Mac App Store or from your own site; both need the [Apple Developer
Program](https://developer.apple.com/programs/).

## Build

```sh
flutter build macos
```

It makes `build/macos/Build/Products/Release/<name>.app`, with Fynkel's
libraries in it. It runs on your Mac as it is. On another it must be signed
and checked by Apple.

A game that uses the network, as for an online leaderboard, needs
`com.apple.security.network.client` in `macos/Runner/Release.entitlements`.

## From your own site

In Xcode, open `macos/Runner.xcworkspace`, choose your team under Signing &
Capabilities, then Product, Archive. In the Organizer: Distribute App,
Direct Distribution. Xcode sends it to Apple to be notarised, and after some
minutes exports an app that opens on any Mac. Zip it, or put it in a disk
image.

## The Mac App Store

The same archive: Distribute App, App Store Connect. The page is made as for
[the App Store](/release/ios/#6-the-page), with a macOS version of the app.
