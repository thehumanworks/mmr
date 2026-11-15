"""Exercise the runner through mock Git/Pixi executables without network access."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import unittest

RUNNER = Path(__file__).with_name("repo-run")
SHA = "a" * 40
MOCK = '''#!{python}
import json, os, pathlib, sys
args = sys.argv[1:]
kind = pathlib.Path(sys.argv[0]).name
with open(os.environ["CALL_LOG"], "a") as stream:
    stream.write(json.dumps({{"tool":kind,"args":args,"cwd":os.getcwd()}}) + "\\n")
if kind == "pixi":
    print("program output")
    sys.exit(int(os.environ.get("PIXI_EXIT", "0")))
if "fetch" in args:
    sys.exit(int(os.environ.get("GIT_EXIT", "0")))
if "init" in args:
    pathlib.Path(args[-1]).mkdir()
if "rev-parse" in args:
    print(os.environ.get("RESOLVED_SHA", "{sha}"))
if "clone" in args:
    path = pathlib.Path(args[-1])
    (path / ".git").mkdir()
    if not os.environ.get("NO_MANIFEST"):
        (path / "pixi.toml").write_text("[workspace]\\n")
    if os.environ.get("WITH_LOCK"):
        (path / "pixi.lock").write_text("lock fixture")
'''


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="repo runner test ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls.jsonl"
        for tool in ("git", "pixi"):
            executable = self.bin / tool
            executable.write_text(MOCK.format(python=sys.executable, sha=SHA))
            executable.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        PIXI_BIN=str(self.bin / "pixi"), CALL_LOG=str(self.log),
                        REPO_RUN_CACHE=str(self.root / "cache with spaces"))

    def run_tool(self, ref=SHA, command=None, **env):
        return subprocess.run([sys.executable, str(RUNNER), f"github.com/example/tool@{ref}",
                               "--", *(command or ["tool", "--version"])],
                              env=dict(self.env, **env), cwd=self.root,
                              text=True, capture_output=True)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_exact_argv_cwd_and_output(self):
        command = ["tool", "file with spaces", "$(touch SHOULD_NOT_EXIST)", "--version"]
        result = self.run_tool(command=command)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "program output\n")
        invocation = self.calls()[-1]
        self.assertEqual(invocation["tool"], "pixi")
        self.assertEqual(invocation["args"][-len(command):], command)
        self.assertEqual(invocation["args"][:2], ["run", "--manifest-path"])
        self.assertEqual(invocation["args"][3:5], ["--executable", "--"])
        self.assertEqual(invocation["cwd"], str(self.root.resolve()))
        self.assertFalse((self.root / "SHOULD_NOT_EXIST").exists())

    def test_pinned_commit_reuses_source_without_git(self):
        self.assertEqual(self.run_tool().returncode, 0)
        first = len(self.calls())
        self.assertEqual(self.run_tool(GIT_EXIT="42").returncode, 0)
        self.assertEqual([c["tool"] for c in self.calls()[first:]], ["pixi"])

    def test_named_ref_refreshes_and_creates_new_checkout(self):
        self.assertEqual(self.run_tool(ref="feature/demo").returncode, 0)
        self.assertEqual(self.run_tool(ref="feature/demo", RESOLVED_SHA="b" * 40).returncode, 0)
        calls = self.calls()
        self.assertEqual(sum("fetch" in c["args"] for c in calls), 2)
        self.assertIn("b" * 40, calls[-1]["args"][2])

    def test_lock_file_enables_locked_resolution(self):
        result = self.run_tool(WITH_LOCK="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--locked", self.calls()[-1]["args"])

    def test_command_exit_status_propagates(self):
        self.assertEqual(self.run_tool(PIXI_EXIT="23").returncode, 23)

    def test_fetch_failure_propagates_without_running_pixi(self):
        result = self.run_tool(GIT_EXIT="42")
        self.assertEqual(result.returncode, 42)
        self.assertNotIn("pixi", [c["tool"] for c in self.calls()])

    def test_missing_manifest_is_clear_error(self):
        result = self.run_tool(NO_MANIFEST="1")
        self.assertEqual(result.returncode, 1)
        self.assertIn("no root pixi.toml", result.stderr)

    def test_real_git_checkout_and_cached_reuse(self):
        real_git = shutil.which("git")
        if not real_git:
            self.skipTest("Git is not installed")
        (self.bin / "git").unlink()
        (self.bin / "git").symlink_to(real_git)
        repo = self.root / "fixture"
        repo.mkdir()
        def git(*args):
            return subprocess.check_output([real_git, "-C", str(repo), *args],
                                           stderr=subprocess.STDOUT, text=True).strip()
        git("init")
        (repo / "pixi.toml").write_text("[workspace]\n")
        git("add", "pixi.toml")
        git("-c", "user.name=Test", "-c", "user.email=test@example.invalid",
            "-c", "commit.gpgsign=false", "commit", "-m", "fixture")
        sha = git("rev-parse", "HEAD")
        config = dict(GIT_CONFIG_COUNT="2", GIT_CONFIG_KEY_0=f"url.{repo.as_uri()}.insteadOf",
                      GIT_CONFIG_VALUE_0="https://github.com/example/tool.git",
                      GIT_CONFIG_KEY_1="protocol.file.allow", GIT_CONFIG_VALUE_1="always")
        result = self.run_tool(ref=sha, **config)
        self.assertEqual(result.returncode, 0, result.stderr)
        # Delete the origin: an immutable cached commit must still work.
        shutil.rmtree(repo)
        result = self.run_tool(ref=sha, **config)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_real_git_annotated_tag_checks_out_tagged_commit(self):
        real_git = shutil.which("git")
        if not real_git:
            self.skipTest("Git is not installed")
        (self.bin / "git").unlink()
        (self.bin / "git").symlink_to(real_git)
        repo = self.root / "tag fixture"
        repo.mkdir()

        def git(*args):
            return subprocess.check_output(
                [real_git, "-C", str(repo), "-c", "user.name=Test",
                 "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false",
                 "-c", "tag.gpgsign=false", *args], stderr=subprocess.STDOUT,
                text=True).strip()

        git("init")
        (repo / "pixi.toml").write_text("[workspace]\n")
        (repo / "marker").write_text("tagged version")
        git("add", ".")
        git("commit", "-m", "tagged version")
        sha = git("rev-parse", "HEAD")
        git("tag", "-a", "v1.0.0", "-m", "release")
        self.assertNotEqual(git("rev-parse", "v1.0.0"), sha)
        (repo / "marker").write_text("newer version")
        git("commit", "-am", "newer version")
        config = dict(GIT_CONFIG_COUNT="2", GIT_CONFIG_KEY_0=f"url.{repo.as_uri()}.insteadOf",
                      GIT_CONFIG_VALUE_0="https://github.com/example/tool.git",
                      GIT_CONFIG_KEY_1="protocol.file.allow", GIT_CONFIG_VALUE_1="always")
        result = self.run_tool(ref="v1.0.0", **config)
        self.assertEqual(result.returncode, 0, result.stderr)
        checkout = Path(self.calls()[-1]["args"][2]).parent
        self.assertEqual(checkout.name, sha)
        self.assertEqual((checkout / "marker").read_text(), "tagged version")
        checked_out_sha = subprocess.check_output(
            [real_git, "-C", str(checkout), "rev-parse", "HEAD"], text=True).strip()
        self.assertEqual(checked_out_sha, sha)

    def test_non_github_urls_rejected(self):
        for url in ("file:///tmp/local", "https://evil.example/owner/repo", "github.com/u/.."):
            with self.subTest(url=url):
                result = subprocess.run([sys.executable, str(RUNNER), url, "--", "tool"],
                                        env=self.env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 1)
        self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main()
