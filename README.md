# Kestrel

A standalone, SDK-first Minecraft client for Android. Kestrel runs a native
Rust engine for Minecraft Java **26.3** and is built around code mods:
`.kmod` WASM plugins, written against a versioned SDK, installed from a
catalog inside the launcher.

**Android 9+ · Vulkan · Touch controls · `.kmod` mod SDK**

[Download the latest release](https://github.com/NepThunder/Kestrel/releases) ·
[Mod catalog](https://github.com/NepThunder/Kestrel-mods) ·
[Build & play](docs/android.md) · [Contribute](CONTRIBUTING_ANDROID.md) ·
[Report a bug](https://github.com/NepThunder/Kestrel/issues/new?template=android-bug.yml)

## Releases

- **Official releases** (tagged `vX.Y.Z`, marked *Latest*) are the stable
  builds: package `com.kestrel.android`, signed with the Kestrel release key.
- **Dev builds** (tagged `kestrel-build-N`) are frequent test builds from the
  development branch: package `com.kestrel.android.dev`, with in-app update
  checks. The two channels install side by side.

## What works

- The full game on your phone: single player and Java multiplayer, Vulkan
  rendering with resolution scaling, resource packs.
- Four touch schemes: joystick or D-pad, each with tap or crosshair aiming.
  Editable button positions, sizes and opacity; handedness, Auto Sprint and
  more.
- A mod SDK with versioned API levels — world, inventory, entity, 3D render,
  light levels, gamma/fog and camera — and a growing catalog of `.kmod` mods
  (HUDs, overlays, Freecam, Freelook, Waypoints and more) that install from
  the launcher's Mods tab.
- Android keyboard, clipboard, AAudio sound and vibration through the
  existing game settings.

Kestrel does not include Minecraft itself: game resources come from Mojang,
and you need your own Minecraft account to play. Fabric/Forge Java mods do
not run in this client; Kestrel mods are `.kmod` plugins built for its SDK.

## Mods

Mods live in [NepThunder/Kestrel-mods](https://github.com/NepThunder/Kestrel-mods):
sources, releases and the catalog index the launcher reads. Every catalog
entry is SHA-256 pinned.

## Lineage

Kestrel is developed on a detached fork of the Horizon engine (now
[Starling](https://github.com/starling-client/starling)), rewritten for
Android as a standalone client. Not affiliated with Mojang or Microsoft.
