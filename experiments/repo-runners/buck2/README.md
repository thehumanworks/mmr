# Buck2 Cargo adapter experiment

Verified on macOS arm64 with Buck2 daily release `2026-11-15`, binary version `2026-11-14-5b3b13d3eacefe325bf971369197e33c5f4b3968`. This builds mmr from the selected checkout's committed source. It does not reuse a prebuilt mmr or invoke Pixi.

```sh
cd experiments/repo-runners/buck2
sh prepare.sh
buck2 run -c experiment.tool_path="/path/to/rust/toolchain/bin:/usr/bin:/bin:/usr/sbin:/sbin" //:mmr -- --version
```

`prepare.sh` archives the repository HEAD into ignored `source.tar`; unchanged bytes do not replace the archive. Thus uncommitted application edits are deliberately excluded. The generic Git runner can prepare a selected checkout, then call this target. The archive is a declared input to Buck2's build action.

The target executes `cargo install --path ... --locked` in fresh scratch directories and returns the resulting executable through Buck2's `RunInfo`. Buck2 owns action execution and output reuse. Cargo owns Rust dependency resolution and compilation. This is an explicit Cargo adapter, **not** a native Buck2 Rust/Reindeer dependency graph.

Mac validation used Rust 1.99.1 from an existing toolchain and the existing Apple compiler/SDK. Cargo dependency downloads happen inside the action. Tool binaries are located by the configured PATH and are not content-tracked by Buck2. Consequently this demonstrates a real local source build, but does not establish hermeticity, remote-execution suitability, downloaded compiler bootstrap or SDK provisioning.

Validation from checkout `5ab497adb9cbf53e240535c55da9d32383dd5888`:

| Check | Result | Wall time |
| --- | --- | --- |
| Fresh build and `--version` | `mmr 0.2.0`, exit 0 | 32.46 s |
| Repeat `--version` | `mmr 0.2.0`, exit 0, no compilation | 0.028 s |
| Read synthetic Codex session | Two expected messages, exit 0 | 0.028 s |

Run the same fixture checks without accessing personal histories:

```sh
python3 verify.py \
  --buck2 /absolute/path/to/buck2 \
  --tool-path "/path/to/rust/toolchain/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  --evidence /tmp/mmr-buck2-evidence
```

The verification uses a temporary HOME containing synthetic records. The first version run can rebuild when the build environment changes; `warm-version` measures a second execution in the same environment. Evidence is in `evidence/macos/`. Parent coordinator owns Linux replay and cross-tool comparison.

A native Buck2 implementation is possible using `rust_library` + `rust_binary`, Reindeer-generated dependency targets, procedural macro dependencies, embedded Markdown inputs and Cargo-version environment variables. This repository has one Cargo package with both `src/lib.rs` and `src/main.rs`. Native translation was not implemented in this adapter experiment; it adds graph/toolchain maintenance without being necessary to prove local source-build execution. Buck2's [toolchain guide](https://buck2.build/docs/rule_authors/writing_toolchains/) permits downloaded toolchains, and [Reindeer](https://github.com/facebookincubator/reindeer/blob/main/docs/MANUAL.md) translates Cargo dependency graphs.

## Final GitHub execution results

The reusable GitHub adapter passed both applications on both hosts. Tested commits: mmr `73663c5d9195aa671ae96f5eb293d42d7a9d74fa`, tny `331c3ce7c43d68edacee32cb4caee71cdee2337b`. All version invocations returned0; final tny binaries passed8 fixture runtime tests on each OS. mmr synthetic read passed on both hosts, and Mac GitHub execution preserved caller working directory. See [final-results.json](evidence/final-results.json) and adjacent logs. Timing is observational, not a comparison across build engines.
