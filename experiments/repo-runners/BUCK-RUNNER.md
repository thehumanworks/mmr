# Experimental GitHub adapter for Buck2

`buck-repo-run` reuses the sibling `repo-run` GitHub checkout/cache implementation
and dispatches an existing Buck2 runnable target. Keep both executable files
beside each other. Requires Python 3.9+, Git, Buck2, and the toolchain required by
the selected repository's build rules. It does not install these prerequisites.

```sh
BUCK2_BIN=/path/to/buck2 ./buck-repo-run \
  --project-dir experiments/repo-runners/buck2 \
  github.com/OWNER/REPO@FULL_COMMIT_SHA -- PROGRAM --version
```

Repeat `--config SECTION.KEY=VALUE` before the repository to pass explicit Buck2
configuration, including toolchain paths required by a project. For example:
`--config "experiment.tool_path=/path/to/rust/bin:/usr/bin:/bin"`. The adapter
does not invent or inject project-specific configuration.

For a root Buck2 workspace, omit `--project-dir`. `PROGRAM` maps to `//:PROGRAM`;
`//package:target` also works. Remaining arguments pass unchanged to the program.
Buck2 starts in the selected project; its `run --chdir` option runs the program
in the caller's working directory. `REPO_RUN_CACHE` selects the shared source
cache. A full commit SHA reuses cached source without fetching it again; named
refs refresh on each invocation. Build outputs remain mutable caches.

If the selected workspace contains `prepare.sh`, the adapter runs it through
`/bin/sh` before invoking Buck2. This is an explicit convention of this adapter,
not a Buck2 feature. The script must be repeatable and should avoid rewriting
unchanged inputs. Both preparation and building execute repository code with the
caller's permissions. Git submodules are not initialized by the source helper.

This experiment does not make arbitrary GitHub repositories runnable, supply
cross-platform toolchains, or remove compilation. Each repository still needs
Buck2 rules that build a runnable target on its host. No production architecture
change is implied.

Run local dispatch checks with `python3 test_buck_repo_run.py`; shared GitHub
cache/checkout checks remain in `python3 test_repo_run.py`. Tests check argument
and working-directory handling, target labels, preparation failure, exit status,
and workspace path validation. Real repository results belong in the evaluation
report; mocked dispatch tests alone do not prove a successful source build.
