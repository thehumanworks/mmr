# Buck2 trial review

Read-only review of tny's `experiments/repo-runners/buck2` and mmr's generic `buck-repo-run` plus its `repo-run` checkout helpers. No application, build adapter or wrapper files changed.

**Verdict:** no blocker found for the scoped experiment: Buck2 can own local source-build actions for both existing build systems, cache their outputs, and run the resulting binaries. The reusable Git wrapper supplies the requested GitHub command shape. This trial does not establish managed cross-platform compiler/SDK provisioning by Buck2.

## Verified by inspection

- Checkout helper resolves branch/tag/HEAD to a commit, clones into a per-commit cache under a repository hash, serializes fetch/creation, and checks full-SHA requests. Named refs refresh; full-SHA cached sources can be reused without fetching. Submodules are not populated.
- Wrapper validates target names and project containment, checks `.buckconfig`, runs optional preparation, forwards configuration and application arguments, propagates exit codes, and uses Buck2 `--chdir` to preserve the caller's runtime directory. It requires the sibling `repo-run` helper, which help text states.
- tny preparation runs Git archive at the repository root. The resulting committed-source archive and build script are declared Buck inputs. An unchanged archive is not replaced. Uncommitted source edits do not enter the build. This is appropriate for the selected-commit Git runner.
- tny extracts into a fresh temporary directory, builds through Make, and copies the resulting executable to the Buck-declared output. The action returns `RunInfo`, so Buck runs that built output. No prebuilt application artifact or Pixi invocation is involved.
- Source/archive, action script and explicit PATH/compiler configuration changes participate in the action identity. Compiler, SDK and other host tool contents do not. The latter is a documented nonhermetic host dependency, not a flaw in the narrower source-build demonstration. Local-only actions correctly avoid claiming remote execution.

## Findings and scope limits

1. **Version reuse limitation:** tny `build.sh` fixes `TNY_VERSION=0.39.0`. A future selected source revision would still display this value. It does not select old source or invalidate the build incorrectly, but version output alone is insufficient evidence of the selected revision. A production/general-use recipe should derive or explicitly parameterize version identity. This does not require restarting the frozen trial.
2. The adapters invoke Cargo/Make inside one Buck action. Buck does not own each Rust crate or C/C++ compilation edge in these experiments. Native Rust/Reindeer or C++ rules would be additional integration, not a prerequisite for this scoped proof.
3. The source commit is pinned; dependencies and toolchains are a separate issue. Crate/source downloads occur inside the actions. Host compiler/SDK changes at an unchanged configured path may reuse an old output. This trial must not be described as hermetic or as equivalent to a pinned toolchain closure.
4. tny's output is the executable only. The adapter does not reproduce every file in `make install` (shell helpers, auxiliary Python scripts, licenses). The reported binary/runtime fixture checks are narrower than complete installed-package equivalence.
5. The wrapper supports repositories that provide the expected Buck workspace and runnable target. It does not infer arbitrary repository build instructions, initialize submodules, or install missing compilers. The generic wrapper is reusable; project build recipes remain necessary.

## Evidence boundary

I directly verified mmr's macOS source build, cached repeat, and synthetic session read earlier. The generic wrapper's recorded mmr macOS cold/warm status files both contain zero. Parent reports tny macOS source build plus eight runtime checks passed. Linux execution is in progress at review time; this review does not independently certify its eventual result. Running Linux under a supplied native NixOS tool environment establishes Linux execution with provided tools, not Buck-managed compiler provisioning. No FHS emulation is needed for that claim.

## Comparison with the original expectation

The trial proves the fetch → selected source → local build → cached executable → run workflow can be assembled with Buck2 and a reusable Git wrapper on the tested hosts. Users do not publish application binaries or maintain GitHub runner images; compilation takes place on the consumer's machine when the local action cache misses.

Buck2 alone does not supply the original bare-GitHub invocation or discover these repositories' toolchain needs. In this trial the wrapper supplies Git checkout and Buck target selection, while host tools supply the compiler/SDK. Pixi's tested recipe additionally provisions its declared tool environment, making it closer to the managed-toolchain part of the request. Buck2 can model downloaded compiler toolchains, but that separate capability was not implemented here. Therefore the fair verdict is **works as an explicit local Cargo/Make adapter; not yet a complete managed cross-toolchain solution**. Native language rules and downloaded toolchains would be further integration, rather than evidence that Buck2 is incapable.
