# Contributing to Horizon Android

Horizon Android is an unofficial Android port of Horizon — Minecraft: Java
Edition 26.3 rewritten in Rust (wgpu) — maintained at [Vasysik/horizon-android](https://github.com/Vasysik/horizon-android).
It is a full fork, not a wrapper around the desktop game: it carries the whole
engine plus an Android shell (launcher, touch controls, gamepad, IME, SAF,
diagnostics, AAudio, DNS, display handling). Read this guide in full before your first contribution. It is the source of truth for how work is accepted here; if it conflicts with `AGENTS.md`, this document wins.

This guide is standalone. You do not need to read any other contributing guide to contribute to Horizon Android: the engine rules that matter for Android work are included below, in Android terms. A short agent guide lives in `AGENTS.md` (with `CLAUDE.md` pointing at it); together, the two files are the complete contribution guide for this project.

## In short

- One issue, one fix, one pull request, under about 500 lines.
- Claim the issue before you start, and work against the `android` branch.
- Prove a fix on hardware that reproduces the bug: device, Android version, GPU, renderer and scale, with before/after evidence.
- Run the Android checks (and `pr_ready.py` for engine code) before opening the PR, or let CI run them. Pasting their output in the PR body is not required.
- If AI-assisted, disclose it in the PR body and tick the checklist box.
- The full rules, with the reasons behind them, are below; if you are short on time, read the one-PR rule and the [pull request checklist](#submitting-pull-request-checklist) first.

## Table of contents

- [In short](#in-short)
- [Prerequisites](#prerequisites)
- [Where to start](#where-to-start)
- [How the code is organised](#how-the-code-is-organised)
- [Vanilla first](#vanilla-first)
- [The one rule : one issue, one fix, one PR](#the-one-rule--one-issue-one-fix-one-pr)
- [Issues: report so the fix can be found](#issues-report-so-the-fix-can-be-found)
- [Claim before you start](#claim-before-you-start)
- [Building and checking](#building-and-checking)
- [Writing tests with signal](#writing-tests-with-signal)
- [Mods and content](#mods-and-content)
- [Device evidence: what counts as proof](#device-evidence-what-counts-as-proof)
- [Regressions: every fix must be traceable](#regressions-every-fix-must-be-traceable)
- [AI-assisted contributions](#ai-assisted-contributions)
- [Upstream and releases](#upstream-and-releases)
- [Review and merge flow](#review-and-merge-flow)
- [How to create a pull request](#how-to-create-a-pull-request)
- [Submitting: pull request checklist](#submitting-pull-request-checklist)
- [What gets closed without review](#what-gets-closed-without-review)
- [Privacy and legal boundaries](#privacy-and-legal-boundaries)
- [Getting help](#getting-help)

## Prerequisites

You need the following. Versions below are as of the current release;
the files and docs linked are the source of truth if they change:

- Rust (the pinned toolchain in `rust-toolchain.toml`; currently 1.95)
- Android SDK 35 and NDK, Gradle 8.14, JDK 25 for the Prepare step and
  JDK 17 for the Gradle build (see `docs/android.md#build-developers-only`
  for the current setup)
- A real Android device or at minimum, a device you can install an APK on.
  An emulator is a development aid, not a test device (see
  [Device evidence](#device-evidence-what-counts-as-proof))
- Your own Minecraft 26.3 installation. Horizon never downloads the game:
  game inputs are prepared from your own copy and stay in `.local/`. Build products and generated game data never enter Git.
- Git, and enough Rust to read the crate you are changing before you change it. Start from the Android baseline, not from a diff against official
  Horizon HEAD: Android pins an older, reviewed upstream baseline, so a raw diff against current official HEAD is not the Android delta.

Useful areas for contribution: touch input, Android lifecycle and save
recovery, accessibility, native hardware controllers, resource import/export, profiling on real phones, and Android-specific rendering or performance work. Rendering and performance reports must include the phone, Android version, renderer, resolution scale, render distance, and APK or source version — a report without those cannot be acted on.

## Where to start

- Target branch is `android`, not `main`. Pull requests against any other
  branch will be asked to retarget.
- Before you start, search open issues and open PRs for the same area. Never duplicate an in-progress contribution; talk to its author first.
- Good first contributions are single, well-described bugs with a reproduction. Large redesigns (a new launcher UI, a new control system, a new settings hub) are not good first contributions, and they are not accepted as a single PR at any experience level — see below.

## How the code is organised

The game workspace is at the repository root (`Cargo.toml`). The Android
shell is in `crates/horizon-client/src/android/` and `packaging/android/`.
File paths below are as of the current release; if a path has moved,
the repository tree is the source of truth.
You do not need to know the whole engine to fix an Android bug, but you
do need to know where your change lives before you make it:

- Android launcher and catalog: `crates/horizon-client/src/android/` (`launcher.rs`, `launcher_pages.rs`, `catalog.rs`, `login.rs`, `wardrobe.rs`, `documents.rs`, `document_store.rs`)
- Android entry point: `crates/horizon-client/src/android_main.rs` (the cdylib that becomes `libhorizon_android.so`, one per ABI)
- Game input: `crates/horizon-client/src/game/pad.rs`, `game/pad_menu.rs`, `game/android_input.rs`; touch controls and the HUD editor in `game/touch*`; standard options widgets in `options_ui.rs`
- Title screens (main menu, world selection, search focus): `crates/horizon-client/src/title.rs` and `title/`
- Rendering and world data: `crates/horizon-client-render/` (`gfx.rs` and `gfx/`, chunks, meshing, light, HUD, menus, settings)
- Gameplay, networking, and protocol: `crates/horizon-client/`, `crates/horizon-world/`, `crates/horizon-protocol/` (protocol 777 for Minecraft 26.3)
- Mods: `crates/horizon-mods/` (`.hmod` content and WASM runtime), with example WASM mods in `mods/official/`
- Android packaging: `packaging/android/` (`AndroidManifest.xml`, `build.gradle`, `java/`), built by `tools/android_build.py` and prepared by `tools/android_prepare.py`
- Tests: `tests/` (Python tests) and Rust unit tests in the crates; parity harnesses in `tools/test_*_parity.py` (see [Vanilla first](#vanilla-first))

Everything players run is Rust. Python and Java are fine for dev tools and harness drivers in `tools/` and for the thin Android Java adapter in `packaging/android/java/`, never for player-facing code.

## Vanilla first

Wherever vanilla Minecraft 26.3 has a feature, vanilla is the specification, down to the order of random calls. "Parity" here means Horizon gives the same results as the pinned 26.3 client and server for the same inputs, checked by a harness: a `tool/test_*_parity.py`
script that runs a small Java oracle against the real game and a Rust probe against Horizon, and compares their outputs.

- Read the vanilla code before writing Horizon's for a gameplay change. `tools/rea_vanilla.py` queries the pinned game bytecode.
  for example: `python3 tools/rea_vanilla.py method net.minecraft.world.level.block.DispenserBlock dispenseFrom`.
  Say in your pull request which vanilla classes and methods you followed.
- Gameplay changes come with a harness, new or extended, so the next change cannot silently break them. Copy a nearby harness.
- Horizon-only features (the Android launcher, the new menus, touch controls) follow the project's design system and are off by default wherever they would change the vanilla look or feel.
- Never delete parity harnesses or tests that pin vanilla behaviour: they are the spec.

For a purely Android change (a launcher button, a SAF import, a controller mapping), a vanilla harness usually does not apply — an Android regression test or a device test plan replaces it (see [Regressions](#regressions-every-fix-must-be-traceable)). Do not claim a parity harness covers an Android-only change when it does not.

## The one rule : one issue, one fix, one PR

**One pull request fixes one issue, or implements one feature, and nothing
else.** A PR that bundles a launcher UI rewrite, a new touch-control
engine, a HUD editor, catalog integration, and several performance and stability fixes in a single PR cannot be reviewed, cannot be bisected,
and cannot be reverted in part when one of its changes regresses. That bundled overhaul is the example of what not to do.

Concretely, a PR must satisfy all of these:

1. **One purpose.** Its title names one change. If you cannot describe
   the PR in one sentence without "and", it is two PRs.
2. **Under about 500 lines of code** (excluding generated files and tests that directly cover the change). Bigger work is split into a series of PRs, each of which builds, passes CI, and makes sense alone, in dependency order. "I will split it later" is not a split.
3. **Atomic commits.** Each commit is one logical change with a message describing only that change. Do not bundle a fix with a refactor, a rename, a formatting pass, or an unrelated drive-by fix in the same commit — or the same PR.
4. **No unrelated files.** Every file in the PR must be needed for its one purpose. If a reviewer has to ask "why is this file here?", the PR is not ready.
5. **One issue link.** The PR body links the single issue it fixes (using the issue's "Fixes" link) or, for a feature, the single approved proposal. A PR that fixes "several issues" fixes none of them traceably.

A PR that combines a UI overhaul with performance work, or a control
system with a renderer change, will be asked to split before review begins, not reviewed as-is in the hope that the good parts survive.

## Issues: report so the fix can be found

Use the Android bug template. A usable report contains:

- App version (release tag or source commit) and device (model, Android version, GPU family — Mali vs Adreno matters, they fail differently)
- Renderer setting (Vulkan, Vulkan compatibility, OpenGL ES), resolution scale, and render distance
- Exact reproduction steps, what happens, and what you expected
- Whether it is a regression, and if so, the last version that worked
- A diagnostics ZIP (Settings > Diagnostic report > Export ZIP) or a logcat excerpt when you can get one and a short video when the bugbbis visual or input-related. A large log dump with no steps is not a substitute for steps.

Do not file a report that only says "fix the shader", "it lags", or
"it crashes" with no version, device, or steps. Maintainers may close
such reports as unactionable and ask for the missing fields.

## Claim before you start

Work is tracked in GitHub issues. An issue that already has an assignee, or that has an open PR linking it, is taken: pick another, or ask its holder in the thread. Before you start, comment `/claim` on the issue. In this fork `/claim` is a plain comment, not a
command: the claim workflow is disabled, so the comment assigns
nothing by itself, it only signals your intent in the thread, and a maintainer may assign the issue to you. No issue for your work yet?
Open one (a line is enough), then post `/claim` on it. `/unclaim` signals that you have given it back. A claim with no open PR (or draft) linking the issue is released after 5 days, open a draft
early so the claim is visible. You can hold 8 open issues at a time, not counting the ones your pull requests already fix. The cap and the 5-day release are maintainer-tracked conventions, not automatic actions.

## Building and checking

Full build instructions live in `docs/android.md#build-developers-only`.
In short, the Android build prepares game resources from your own Minecraft 26.3 copy, then builds one APK per ABI. The standard checks before asking for review are:

```sh
python3 tools/android_prepare.py --java-home "$JAVA_HOME"
python tools/android_build.py --abi arm64-v8a --clippy
python tools/android_build.py --abi arm64-v8a
python -m unittest discover -s tests -p test_android_release_audit.py
python tools/check_public_tree.py
```

For engine code, also run the self-check where it applies. The base
is the `android` branch you are targeting, not official Horizon's
`main` — a diff against current official HEAD is not the Android delta:

```sh
git remote add vasysik https://github.com/Vasysik/horizon-android.git   # once, if you do not already have a remote for it
python3 tools/pr_ready.py --base origin/android   # in the main repository
python3 tools/pr_ready.py --base vasysik/android  # from a fork
```

`pr_ready.py` looks at what your branch changes and runs what
matters: rustfmt, clippy with warnings denied, the tests of the crates you touched, generated files, `Cargo.lock`, the Python tests, and the vanilla harnesses your change can affect. Without the vanilla data on your machine it lists those harnesses instead; CI runs them.
`--quick` skips the harnesses. It exits non-zero when something fails.

APK variants, in plain terms:

- A normal local build produces a debuggable APK with a locally generated debug keystore. That is the build for testing, not for distribution.
- `--release-candidate` produces a non-debuggable local APK, remaps personal build paths, and checks known credential patterns and packaged data paths. That checks privacy, not permission to redistribute bundled Minecraft resources.
- `--public` is for distribution: it filters the generated table bundle, downloads original world data and textures from Mojang on the phone, and disables automatic agent control. See `docs/android-publication.md`.
- Releases ship split APKs, one per ABI (arm64-v8a, armeabi-v7a, x86_64, x86), not a universal APK. A debug build for reporter testing must therefore be built for the reporter's ABI, an arm64 build is useless to a 32-bit-only TV.

Keep your runs away from real player data. Any game or launcher you
start for testing gets fresh scratch folders (`HORIZON_HOME`,
`HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, plus
`HORIZON_LAUNCHER_NO_SELF_UPDATE=1` and `HORIZON_NO_AUDIO=1` where
they apply). Never write test data to
`~/.local/share/{applications,horizon,horizon-launcher}` or
`~/.local/bin`.

## Writing tests with signal

- Test observable behaviour, not words in docs, comments, or source.
  Keep checks for machine-consumed contracts.
- Use independent expected values or a vanilla oracle; do not derive
  the answer from the code under test.
- Test outcomes, not only calls to stubs. Use mocks to isolate
  behaviour, not as the behaviour itself.
- Skip tests of trivial constants, derives, and getters, or behaviour
  already covered by another test.
- Avoid pinning implementation details; a harmless refactor should
  not break a behaviour test.
- Make slow tests faster without removing cases or weakening their
  coverage.

## Mods and content

Users and contributors constantly confuse the mod formats, so the
rules are stated here once, plainly:

- `.hmod` is a content mod or a sandboxed WebAssembly mod. It is the
  only mod format Android installs, from the catalog or via Import
  file, and it appears in Installed to be enabled.
- `.hnmod` is a native mixin source bundle compiled into a new game
  binary on desktop. Android does not install it: the catalog shows
  ".hnmod: later", and Import rejects it. It cannot be accepted by
  importing it into the installed app, because the app cannot rebuild
  and re-sign itself on a phone. The routes that work are a PC-built
  modded APK (the mod baked in, not toggleable) or a WASM `.hmod`
  port by the mod author.
- Java Fabric, Forge, and NeoForge mods never run in Horizon, on any
  platform. Do not claim otherwise, and do not accept a `.jar` as a
  mod in Android code.
- WASM `.hmod` mods require a 64-bit APK (Wasmtime is omitted from
  32-bit builds). Content-only `.hmod` mods work on any ABI.
- Content import on Android goes through SAF: Import file uses the
  system document picker, and Open folder uses the app's
  DocumentsProvider view of its content folders. Neither needs a
  storage permission, and neither is a raw filesystem path. A file
  that is copied in manually still has to be a valid `.hmod`, and it
  still has to be enabled in Installed — import does not auto-enable.

Run the checks yourself where you can; CI also runs them, and
pasting their output in the PR body is not required. A PR with
failing CI is not reviewed. If a check cannot run on your machine
(for example, a harness needing vanilla data), say so explicitly
in the PR — and do not claim it passed.

## Device evidence: what counts as proof

This project has been burned repeatedly by claims that were not tested
on the hardware that fails. The following are rules, not suggestions:

- **A fix is not "verified" until it is tested on a device that can
  reproduce the problem.** An emulator result, a parser result, a unit
  test alone, or a test on a different GPU family is a hypothesis, and
  must be labelled as one in the PR.
- **Emulator FPS does not establish phone performance.** Nor does a
  desktop build, nor a test on Adreno establish a fix for Mali (or the
  reverse). If you cannot test on the failing hardware, say so, and
  ask the issue reporter to test a debug build — do not claim the fix.
- **A debug build for reporter testing is a normal step, not a release.**
  It must be built from the PR branch by CI, linked in the issue, and
  labelled with what it contains and what it does *not* isolate. If
  the build contains other fixes, say so: a test of a combined build
  cannot prove a single fix.
- **Before/after evidence is required for anything players can see or
  feel**: screenshots or video for UI, and a measured before/after
  (FPS, frame time, or the specific symptom) for performance. "Feels
  faster" is not evidence.
- **Do not present an indirect result as a confirmed result.** If the
  only evidence is indirect, the PR description and any issue comment
  must say so in plain words.

## Regressions: every fix must be traceable

A regression is only fixable if the change that caused it can be found.
Therefore:

- Every bug fix must either add or extend a regression test that fails
  without the fix, or state in the PR why no test is possible and give
  the manual test plan that replaces it. "No test, tested manually"
  with no steps is not a plan.
- Every behaviour change must note, in the PR, what could regress and
  how you checked it. For input changes: keyboard, touch, and controller
  paths separately. For renderer changes: Vulkan, Vulkan compatibility,
  and OpenGL paths, and Mali and Adreno where you can test them.
- A fix must not silently change unrelated behaviour. If you discover
  a second bug while fixing the first, file it as a second issue and
  fix it in a second PR.
- Release notes must be able to answer, for every shipped change,
  "which PR, which issue, what changed, what was tested". A PR with no
  issue link and no test evidence cannot be written up, and will not
  be merged on that ground alone.

## AI-assisted contributions

AI assistance is welcome, and it is held to exactly the same rules as
human work — plus disclosure, because a reviewer cannot judge a change
whose origin is hidden:

- The PR body starts with a line naming the person whose agent opened
  it and the tool:
  `> 🤖 Opened by <login>'s AI agent (<tool, model>), not by <login> personally.`
  Issues say "Filed by", reviews say "Review by", and every agent
  comment starts with `🤖 Agent:`.
- If the PR is AI-assisted, the PR checklist's "This PR is
  AI-assisted" box is ticked, and a human has reviewed every line.
  The human who opens the PR is responsible for it, in full, whether
  they typed it or not.
  "The AI wrote it" is never an answer to a review question.
- An AI agent must follow `AGENTS.md` in full. In particular, an agent
  must not open a PR it has not disclosed, must not claim a fix it has
  not device-tested, and must stop and ask its human to split the work
  when a task grows beyond one PR.
- A PR opened by an agent that does not disclose that it is
  agent-opened will be closed, not merely asked to add a line.

## Upstream and releases

This fork preserves upstream authorship and history. Do not copy
engine files from upstream Horizon by hand: for upstream fixes,
review and merge the reviewed upstream history into `android`,
resolve Android module paths and platform bridges explicitly, keep
upstream deployment workflows inactive, and update
`docs/android-upstream.json`. Never merge experimental upstream
branches automatically, and never claim a raw diff against current
official HEAD is the Android delta — Android pins an older, reviewed
upstream baseline.

Releases are tagged Android releases (currently the 0.7.x
series), with split APKs per ABI and release notes that must be
able to answer, for every shipped change, "which PR, which issue,
what changed, what was tested".
Merges ship in release batches. A PR is not a release, and a debug
build is not a release: do not tell users a fix is "in" until it is
in a tagged release. If a change needs a release note, the PR body
must contain the one-sentence version of it.

## Review and merge flow

1. Author opens a PR against `android`, fills in the template, ticks
   "Allow edits by maintainers", and links its single issue.
2. CI must be green. A maintainer reviews the code, not just the
   description. At least one human maintainer approval is required;
   an AI approval, an AI review, or the author's own approval is not
   a review.
3. Small fixups (a lint, a rebase, a nit) may be pushed by the
   maintainer directly to the PR branch. Design questions go back to
   the author.
4. Once a maintainer has pushed to your branch, merge `android` into
   it; do not rebase or force-push. A force-push erases the maintainer's
   fixups and restarts review.
5. The maintainer merges. Authors do not merge their own PRs, and no
   PR is merged by its own agent.
6. Merges ship in release batches. A PR is not a release, and a debug
   build is not a release: do not tell users a fix is "in" until it
   is in a tagged release.

If a PR has been open with no review for a long time, a polite bump in
the PR thread is fine. Opening a duplicate PR to get attention is not.

## How to create a pull request

This is the standard fork-and-pull-request workflow. Follow it
exactly, even if you have push access somewhere else: you do not
commit directly to this repository, and you do not open a PR from
the `android` branch itself.

**1. Fork the repository.** On GitHub, open
Vasysik/horizon-android and click Fork. That creates your own copy
under your account. All of your work goes there first.

**2. Clone your fork, and add the main repository as a remote.**
The remote is named `vasysik` here on purpose: in most Horizon
docs `upstream` means official Horizon, which is a different
repository from this fork.

```sh
git clone https://github.com/<your-login>/horizon-android.git
cd horizon-android
git remote add vasysik https://github.com/Vasysik/horizon-android.git
git fetch vasysik
```

If you prefer a Git GUI (GitHub Desktop, Android Studio's Git
integration, or similar), the steps are the same — fork, clone
your fork, add the main repository as a remote — only the buttons
differ.

**3. Start from an up-to-date `android` branch, then make a new
branch for this one change.** One branch per PR, one PR per issue.
Name the branch short and descriptive, for example
`fix-chat-keyboard` or `touch-shield-transform`.

```sh
git switch -c <branch-name> vasysik/android
```

**4. Make the change, test it, and commit it.** Keep commits
atomic: one logical change per commit, with a message describing
only that change. Run the checks in
[Building and checking](#building-and-checking) before you push,
and test on a device where the bug reproduces (see
[Device evidence](#device-evidence-what-counts-as-proof)).

**5. Before you open the PR, check that the branch contains only
your work.** This is the step that catches the most common beginner
mistake — a branch that also contains somebody else's PR, because
it was started from the wrong branch:

```sh
git log --oneline vasysik/android..HEAD
git diff --stat vasysik/android...HEAD
```

The log should list only your commits, and the diff stat should
match what your PR body describes. If it lists work you did not
write, stop: start a fresh branch from `vasysik/android` and move
only your commits onto it, instead of opening a PR that drags the
other work in with yours.

**6. Push the branch to your fork, then open the PR.** Pushing
does not create a PR by itself:

```sh
git push -u origin <branch-name>
```

Then, on GitHub, open a pull request with base repository
Vasysik/horizon-android, base branch `android`, and head
repository your fork, head branch `<branch-name>`. (With the GitHub
CLI, `gh pr create --base android` from your branch does the same
thing.) If the work is not ready for review yet, open it as a draft
— that also makes your claim visible, as described in
[Claim before you start](#claim-before-you-start).

**7. Fill in the PR template completely.** Do not leave it blank
and do not delete sections without a reason. At minimum:

- What the change is and why, in one paragraph.
- The single issue it fixes, linked in the body.
- The kind of change (bug, feature, perf, tooling, docs).
- The vanilla reference, or "Horizon-only" / "none" where that
  applies.
- What checks you ran, if you ran any — pasting the full output
  is not required, and CI also runs the checks.
- Before/after screenshots or video for anything players can see.
- If an AI agent opened the PR, the disclosure line as its first
  line (see [AI-assisted contributions](#ai-assisted-contributions)).
- "Allow edits by maintainers" ticked, so a maintainer can push
  small fixups without a round trip.

Then work through the
[Submitting: pull request checklist](#submitting-pull-request-checklist)
below and tick every box that applies. An unticked box needs an
explanation in the body, not silence.

**8. After opening, keep the PR on the same branch.** Review
comments are answered by pushing more commits to that branch; the
PR updates automatically. Do not open a second PR for the same
change, do not force-push after a maintainer has pushed to your
branch (see [Review and merge flow](#review-and-merge-flow)), and
do not close and reopen the PR to hide its history.

Common mistakes that get a PR closed without review: opening it
against `main` instead of `android`, opening it from your fork's
`android` branch (which then contains everything on that branch),
including another author's work, leaving the template empty, and
bundling several fixes into one PR. All of them are avoidable by
steps 3, 5, and 7 above.

## Submitting: pull request checklist

Copy this into your PR and tick every box that applies. An unticked
box needs an explanation in the PR body, not silence.

- [ ] One issue, one fix: this PR fixes exactly one issue (linked
      above) and contains no unrelated change
- [ ] Under ~500 lines of code, or split into a series of PRs each of
      which stands alone
- [ ] Issue claimed before work started; no duplicate open PR for the
      same issue
- [ ] No Mojang files, tokens, signing keys, account or session files,
      or `.local/` data committed
- [ ] Screenshots or video for anything players can see
- [ ] This PR is AI-assisted (if so, disclosure line present, see
      [AI-assisted contributions](#ai-assisted-contributions), and a
      human has reviewed every line)
- [ ] "Allow edits by maintainers" ticked

## What gets closed without review

A maintainer may close, rather than review, a PR that:

- bundles multiple fixes or features, or exceeds the size rule with no
  split plan, after one request to split;
- has no linked issue;
- is opened by an agent without disclosure;
- contains committed game files, credentials, or private user data;
- duplicates an in-progress PR for the same issue.

Closing in these cases is not a judgement on the author's skill. It is
how the project stays reviewable: a PR that cannot be reviewed in a
reasonable time cannot be merged safely, however good its code may be.

## Privacy and legal boundaries

- Never commit Mojang's files: game jars, assets, decompiled source,
  or prepared resources (`.local/`). `tools/check_public_tree.py`
  rejects them, and the pre-push hook checks the full history of the
  branches and tags being pushed. Run
  `python3 tools/check_public_tree.py --history` to audit local refs.
- Never commit signing keys, tokens, passwords, account or session
  files, or complete app data — yours or anyone else's.
- Only original code, or code whose licence allows it. Allowed:
  MIT, MIT-0, BSD-2-Clause, BSD-3-Clause, 0BSD, ISC, zlib,
  Apache-2.0, Unlicense, and the CC0-1.0 public-domain dedication.
  Everything else is off limits, even to read: GPL, LGPL, AGPL, MPL,
  EPL, Polyform, the Creative Commons licences (CC BY, CC BY-SA,
  CC BY-NC and the others; CC0 is a public-domain dedication, not one
  of them), custom or "source available" licences, all rights
  reserved, and any project with no licence or an unclear one. Do
  not read, copy, translate, or paraphrase their source, and do not
  send code an AI tool wrote from it. Being able to download a mod
  or a pack does not give you the right to read its files.
- Check the licence yourself. Read the LICENSE file at the commit
  you use and the header of each file you take: some repositories
  mix licences, and a file under another licence is off limits. A
  licence that changed: the one at your commit counts. In doubt,
  don't — ask in the issue thread first.
- Say it in the pull request: the project, a link to the commit, its
  licence, and what you read or copied. A pull request that used
  another project's source without saying so is closed.
- Copied or adapted code keeps its copyright line and licence notice
  at the top of the file, and gets an entry in
  `THIRD_PARTY_NOTICES.md`. Apache-2.0 code also keeps its NOTICE
  text and says what was changed.
- User documentation and file formats are fine to read, and packs
  and mods are fine to run and observe, whatever their licence.
- Do not post other users' private server details, account data, or
  diagnostics that contain them. Redact before posting a log or a
  diagnostics ZIP.

## Getting help

Ask in the issue thread for the area you are working on, or in the
[Android Discord](https://discord.gg/D4Yx3Acv3f), not in a private
message to a maintainer. If you are new, say so, and say what you
have already tried — a question with a reproduction and a pointer to
the code you read gets an answer far faster than "it doesn't work".
Contributors are happy to help with a specific, well-described
problem in this codebase, though the prerequisites listed above are
expected knowledge.
