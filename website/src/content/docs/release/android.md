---
title: "Google Play"
description: "Sign, build and upload for Android."
---

You need a [Google Play developer
account](https://play.google.com/console/signup), and to have done [Before
you release](/release/before/).

## 1. Make a key

Once, for all your games. Keep the file and its password somewhere safe, and
out of git.

```sh
keytool -genkey -v -keystore ~/upload-keystore.jks -keyalg RSA \
  -keysize 2048 -validity 10000 -alias upload
```

## 2. Sign with it

Make `android/key.properties` (Flutter's `.gitignore` leaves it out already):

```properties
storePassword=<the password>
keyPassword=<the password>
keyAlias=upload
storeFile=/Users/you/upload-keystore.jks
```

and in `android/app/build.gradle.kts`:

```kotlin
import java.io.FileInputStream
import java.util.Properties

plugins { /* as it was */ }

val keys = Properties().apply {
    val file = rootProject.file("key.properties")
    if (file.exists()) load(FileInputStream(file))
}

android {
    signingConfigs {
        create("release") {
            keyAlias = keys.getProperty("keyAlias")
            keyPassword = keys.getProperty("keyPassword")
            storeFile = keys.getProperty("storeFile")?.let { file(it) }
            storePassword = keys.getProperty("storePassword")
        }
    }
    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("release") // was "debug"
        }
    }
}
```

## 3. Build

Fynkel runs on 64-bit phones, so leave 32-bit ones out. Otherwise Play would
offer the game to phones it can't start on:

```sh
flutter build appbundle --target-platform android-arm64,android-x64
```

It makes `build/app/outputs/bundle/release/app-release.aab`. To try it on a
phone first, build an APK of the same: `flutter build apk --target-platform
android-arm64`, then `adb install`.

## 4. Upload

In the [Play Console](https://play.google.com/console), make the app, and a
release under Testing, Internal testing. Upload the `.aab`, and add your own
address as a tester: you get it from Play within minutes, as players will.

A new personal account must run a closed test before it can publish; the
Console says with how many people, and for how long.

## 5. The page

- **Data safety.** Fynkel collects nothing and sends nothing. Answer for your
  game and for the other packages it uses.
- **Content rating**, screenshots, a 512 × 512 icon and a 1024 × 500 picture.
- **Devices.** Play offers the game to phones with Android 11 or later and a
  64-bit processor; the list is under Reach and devices.

Then promote the release from testing to production.
