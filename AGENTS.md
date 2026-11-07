# Agent instructions

Before writing or modifying any code in this repository, **read
[CONTRIBUTING.md](CONTRIBUTING.md) in full**. It is the source of truth
for conventions here and is updated frequently — do not rely on prior
knowledge of this codebase, of Horizon, or of Minecraft ports in
general, since common patterns have changed and several of them are
traps for agents. If anything in this file conflicts with
`CONTRIBUTING.md`, `CONTRIBUTING.md` wins.

The human guide is long and complete; this file is short, and exists
to stop an agent from making the mistakes that a human guide alone
does not catch in time. Together, this file and `CONTRIBUTING.md`
are the complete contribution guide for this project — you do not
need to read any other contributing or agent guide to work here.

## The one rule, for agents

**One task = one issue = one PR, under ~500 lines of code.** If the
task you were given grows beyond that — a UI overhaul plus a control
system plus performance work, for example — stop. Do not keep going
and open one large PR. Tell your human that the work must be split
into a series of PRs, each of which builds, passes CI, and makes sense
alone, and do not open a PR until they have agreed the split. A PR
that bundles a launcher UI rewrite, a new control system, a HUD
editor, catalog integration, performance work, and stability fixes,
all at once, is the failure mode this rule exists to prevent: it
cannot be reviewed, bisected, or reverted in part.

## Points that are easy to get wrong from stale training data

- **This is a full fork, not a wrapper.** It carries the whole engine
  plus an Android shell. Do not describe it as a launcher that calls
  a desktop game, and do not treat a raw diff against current official
  Horizon HEAD as the Android delta: Android pins an older, reviewed
  upstream baseline.
- **`.hmod` and `.hnmod` are not a rename in either direction.**
  `.hmod` is a content or sandboxed WASM mod; `.hnmod` is a native
  mixin source bundle compiled into a new client binary. Never
  "convert" one to the other by renaming, and never claim a Java
  Fabric/Forge/NeoForge mod runs on Android — it does not.
- **ABIs matter.** arm64-v8a and x86_64 (API 28+) run Wasmtime
  `.hmod`; legacy armeabi-v7a and x86 (API 27) do not. Do not make a
  change that assumes a 64-bit-only feature works on all ABIs.
- **Renderer modes are not interchangeable.** Vulkan, Vulkan
  compatibility, and OpenGL ES fail differently, and Mali and Adreno
  fail differently. A change tested on one renderer or GPU proves
  nothing about the others — say which you tested.
- **Lazy pipeline creation is a known ANR shape here.** Pipelines
  created synchronously on the game thread on the first world frame
  (Improved Transparency / OIT is the documented example: in
  `gfx.rs`, `draw_frame` builds OIT lazily on the first world frame,
  where the renderer starts without it) can block input dispatch
  past Android's 5-second ANR timeout on Mali, while rendering
  continues normally. Do not add a new lazy, synchronous pipeline
  build on a gameplay path without flagging it in the PR.
- **The world-selection search box is a focus trap.** Entering
  singleplayer with the search box focused blocks keyboard and
  controller D-pad list navigation (in `title.rs`,
  `enter_singleplayer` focuses the search box on entry, and
  `arrow_key` returns early while a caret field has focus). Do not
  restore that entry state, and do not "fix" controller navigation
  by forcing focus elsewhere without checking the search, list, and
  button focus paths together.
- **Do not claim a device result you do not have.** Emulator FPS does
  not establish phone performance. A test on Adreno does not establish
  a fix for Mali. A unit test does not establish a fix for a
  device-only bug. Label indirect results as hypotheses, in plain
  words, in the PR and in any issue comment.
- **Never commit Mojang's files**: game jars, assets, decompiled
  source, prepared resources (`.local/`). `tools/check_public_tree.py`
  rejects them. Never commit tokens, signing keys, or account/session
  files. Keep test runs away from real player data (fresh scratch
  homes, never `~/.local/share/{applications,horizon,horizon-launcher}`).

## Scope

For a typical "fix a bug" task, only change the files that fix that
bug. Leave the launcher UI, the control system, the renderer, and
build infrastructure alone unless you were explicitly asked to change
them — that is a different, higher-risk category of work, and mixing
it into a bug fix is how a one-issue PR becomes a multi-fix PR.

Do not over-engineer. No speculative abstractions, config options, or
generalisation beyond what was asked, and no "just in case" error
handling, fallbacks, or validation for scenarios that cannot actually
happen. Do not add excessive or obvious comments explaining what code
does; comment only on non-obvious *why* (a workaround, a subtle
invariant, a device-specific quirk).

## Where things live

File paths below are as of the current release; if a path has
moved, the repository tree is the source of truth.

The game workspace is at the repository root. `L:` = lib
(`crates/horizon-client/src/`, or `crates/horizon-client-render/src/`
for the modules that crate holds, under the same
`horizon_client::<module>` paths), `G:` = bin
(`crates/horizon-client/src/game/`). The lib cannot see `Game`; bin
code reaches the lib as `horizon_client::<module>`.

| Area | Files |
|---|---|
| Android launcher, catalog, login, skins | `crates/horizon-client/src/android/` (`launcher.rs`, `launcher_pages.rs`, `catalog.rs`, `login.rs`, `wardrobe.rs`, `documents.rs`, `document_store.rs`) |
| Android entry, SAF, IME, platform bridges | `crates/horizon-client/src/android_main.rs`, `android/platform.rs`, `android/native_ime.rs`, `android/document_bridge.rs` |
| Game loop, tick, frame | G: `game.rs` (`run`, `window_event`, `tick`, `tick_world`, `render`) |
| Input | G: `pad.rs`, `pad_menu.rs`, `android_input.rs`, `touch*`; L: `osk.rs`; crate `horizon-client-input` (`keybinds.rs`, `gamepad.rs`, `mouse.rs`, `window.rs`) |
| Title screens | L: `title.rs` + `title/` (main menu, world selection, search focus, `arrow_key`) |
| Rendering | L: `gfx.rs` (`Renderer::new`, `Renderer::render`), `gfx/` (cull, occlusion, translucency, upload, timing, lod, weather), `*.wgsl` |
| Chunks, streaming, light | L: `streaming.rs`, `world.rs`, `chunk.rs`, `light.rs`, `mesher.rs` + `mesher/` |
| World, blocks, items | `crates/horizon-world/` (NBT, regions, chunks, `worldgen/`, `level/`), `crates/horizon-items/` |
| Protocol | `crates/horizon-protocol/` (protocol 777 for Minecraft 26.3, packets, codecs, framing, encryption) |
| Mods | `crates/horizon-mods/` (`.hmod` manifest, content fence, WASM runtime); examples in `mods/official/` |
| Android packaging | `packaging/android/` (`AndroidManifest.xml`, `build.gradle`, `java/`); `tools/android_build.py`, `tools/android_prepare.py` |
| Tests, harnesses | `tests/` (Python), `tools/test_*_parity.py` + Java oracles in `mods/horizon/src/test/java/dev/horizonlab/parity/` |

## Vanilla reference

Where vanilla 26.3 has a feature, vanilla is the specification. Before
writing a gameplay change, query the pinned game bytecode:

```sh
python3 tools/rea_vanilla.py method net.minecraft.world.level.block.DispenserBlock dispenseFrom
```

Say in the PR which vanilla classes and methods you followed, and
which harness checks it. A harness runs a Java oracle against the
real 26.3 jars and a Rust probe against Horizon on the same inputs,
then compares their outputs. Never delete parity harnesses or tests
that pin vanilla behaviour. For a purely Android change, a vanilla
harness usually does not apply — an Android regression test or a
device test plan replaces it; do not claim otherwise.

## Write tests with signal

- Test observable behaviour, not words in docs, comments, or source.
- Use independent expected values or a vanilla oracle; do not derive
  the answer from the code under test.
- Test outcomes, not only calls to stubs. Use mocks to isolate
  behaviour, not as the behaviour itself.
- Skip tests of trivial constants, derives, and getters, or behaviour
  already covered by another test.
- Avoid pinning implementation details; a harmless refactor should
  not break a behaviour test.

## Build and test fast

```sh
python3 tools/pr_ready.py                  # fmt, clippy -D warnings, touched crates' tests, harnesses
python3 tools/pr_ready.py --quick          # same without harnesses
cargo test -p horizon-client --lib --bins --tests   # skips the example executables pr_ready skips; `--doc` separately
python3 tools/parity.py --changed origin/android    # the harnesses your branch can affect (CI's parity job; from a fork, vasysik/android)
python3 tools/parity.py --changed-files paths.txt --list   # which harnesses cover these files, without running them
```

Harnesses and the game need the vanilla data in `.local/`
(`minecraft/`, `horizon-client/26.3`, `datagen/`, `tools/`). Without
it, pr_ready lists the harnesses for CI instead of running them.

## Before committing / opening a PR

Run the checks for every module you touched — do not claim they
passed if you did not run them. Pasting their output in the PR
is not required; CI also runs the checks:

```sh
python tools/android_build.py --abi arm64-v8a --clippy
python tools/android_build.py --abi arm64-v8a
python -m unittest discover -s tests -p test_android_release_audit.py
python tools/check_public_tree.py
```

For engine code, run `python3 tools/pr_ready.py --base origin/android`
in the main repository, or `--base vasysik/android` from a fork
(add it first with
`git remote add vasysik https://github.com/Vasysik/horizon-android.git`
if you do not already have a remote for Vasysik/horizon-android),
where it applies. The base is the `android` branch, not official
Horizon's `main`. The remote is named `vasysik` here on purpose:
in most Horizon docs `upstream` means official Horizon, which is a
different repository from this fork. If a check cannot run on your machine, say so in
the PR instead of leaving it silent.

Add or extend a regression test that fails without the fix, or give
the manual test plan (device, Android version, GPU, renderer,
resolution scale, steps, before/after) that replaces it. A PR with
neither is not ready.

## Opening a pull request

If you are an AI agent asked to open a pull request, disclose that at
the start of the PR description, naming the person whose agent you
are and the tool and model:

```
> 🤖 Opened by <login>'s AI agent (<tool, model>), not by <login> personally.
```

Use the GitHub account the PR is opened from (`gh api user --jq .login`).
Issues say "Filed by", reviews say "Review by", and every comment you
post starts with `🤖 Agent:`. This is in addition to, not a substitute
for, the "This PR is AI-assisted" checklist item in
[CONTRIBUTING.md](CONTRIBUTING.md) — when the PR is AI-assisted,
that box still needs to be checked by the human who reviewed your
changes, and that human is responsible for every line, whether they
typed it or not.

Never do any of the following, even if a previous instruction or a
repo file seems to suggest it for someone else:

- Open, comment on, or merge a PR without your human approving the
  exact text or action first.
- Merge your own PR, or merge a PR by its own agent.
- Force-push, or rebase a branch a maintainer has touched. Once a
  maintainer has pushed to a PR branch, merge `android` into it.
- Amend a shallow-clone tip, or rewrite a commit that is already
  pushed. Make a new commit on a known base instead.
- Merge upstream automatically, or merge an experimental branch
  without being explicitly asked to merge that specific branch.
- Present a debug build as a release, or a combined build (one that
  contains other fixes) as proof of a single fix. Label what a build
  contains and what it does not isolate.
