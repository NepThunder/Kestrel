# Contributing to Kestrel

Kestrel is a standalone Minecraft client for Android, developed on a
detached fork of the Horizon engine. The `android` branch is the main
branch; day-to-day development happens on `kestrel-rebrand` and merges
into `android` for releases.

Start with small changes. Read [CONTRIBUTING.md](CONTRIBUTING.md) and
[AGENTS.md](AGENTS.md) for the engine's rules — the short version is that
vanilla behaviour is the specification wherever vanilla has the feature.

Useful contributions include touch input, Android lifecycle and save
recovery, accessibility, gamepad support, the mod SDK and its catalog,
resource import/export, and profiling on real phones. For rendering or
performance reports, include the phone, Android version, renderer,
resolution scale, render distance and the build you ran. Emulator FPS
does not establish phone performance.

[Report a bug](https://github.com/NepThunder/horizon-android/issues/new?template=android-bug.yml)
or send a focused pull request against `android`. Reproduction steps and a
short video often help more than a large log dump. Do not post
account/session files, tokens, private server details, signing keys or
complete app data.

See the [Android build instructions](docs/android.md#build-developers-only)
for the SDK/NDK setup. Game inputs must be prepared from your own
Minecraft installation and stay in `.local/`. Build products and generated
game data never enter Git.
