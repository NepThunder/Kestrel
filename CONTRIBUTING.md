# Contributing to Kestrel

Kestrel is Minecraft: Java Edition 26.3 rewritten in Rust, shipped as a
standalone Android client. The rule that shapes everything is that **it
plays exactly like vanilla** wherever vanilla has the feature; Kestrel's
own additions (the mod SDK, the catalog, the Android experience) sit on
top of that base.

This repository holds the game, the launcher and the mod runtime, plus
the tools, tests and docs that build, check and release them.

- Android work: start with [CONTRIBUTING_ANDROID.md](CONTRIBUTING_ANDROID.md).
- Repository rules for anyone (human or agent) working in the tree:
  [AGENTS.md](AGENTS.md).
- Bugs go to [issues](https://github.com/NepThunder/Kestrel/issues);
  use the Android bug template and include your build and reproduction
  steps.

## Pull requests

- Keep them small and focused (under ~500 lines where you can).
- Run the self-check before asking for a review:
  `python3 tools/pr_ready.py` (or `--quick` without the harnesses) and
  paste its summary into the PR.
- Never commit Mojang's files: game jars, assets, decompiled source or
  prepared resources. `tools/check_public_tree.py` rejects them.
- Test observable behaviour, ideally against a vanilla oracle; do not
  derive expected values from the code under test.
