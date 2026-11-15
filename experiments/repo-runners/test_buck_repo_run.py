"""Local adapter tests; Git checkout behavior is covered by test_repo_run.py."""
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

loader = importlib.machinery.SourceFileLoader('buck_repo_run', str(Path(__file__).with_name('buck-repo-run')))
spec = importlib.util.spec_from_loader(loader.name, loader)
runner = importlib.util.module_from_spec(spec)
loader.exec_module(runner)


class BuckRepoRunTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='buck adapter test ')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / ('a' * 40)
        self.project = self.source / 'nested project'
        self.project.mkdir(parents=True)
        (self.project / '.buckconfig').write_text('[cells]\nroot = .\n')
        self.log = self.root / 'buck.json'
        self.buck = self.root / 'buck2'
        self.buck.write_text('#!' + sys.executable + '\n' +
            'import json, os, pathlib, sys\n' +
            'pathlib.Path(os.environ["BUCK_TEST_LOG"]).write_text(json.dumps({"args":sys.argv[1:],"cwd":os.getcwd()}))\n' +
            'sys.exit(int(os.environ.get("BUCK_TEST_EXIT", "0")))\n')
        self.buck.chmod(0o755)
        self.helpers = runner.load_checkout_helpers()
        self.checkout = self.enterContext(patch.object(self.helpers, 'checkout', return_value=self.source))
        self.enterContext(patch.object(runner, 'load_checkout_helpers', return_value=self.helpers))
        self.enterContext(patch.dict(os.environ, BUCK2_BIN=str(self.buck), BUCK_TEST_LOG=str(self.log)))

    def run_adapter(self, command=None, project='nested project'):
        return runner.main(['--project-dir', project, 'github.com/example/tool@' + 'a'*40,
                            '--', *(command or ['tool', '--version'])])

    def test_preserves_arguments_and_caller_cwd(self):
        self.assertEqual(self.run_adapter(['tool', 'argument with spaces', '$(touch nope)']), 0)
        call = json.loads(self.log.read_text())
        self.assertEqual(call['args'], ['run', '--chdir', os.getcwd(), '//:tool', '--',
                                         'argument with spaces', '$(touch nope)'])
        self.assertEqual(call['cwd'], str(self.project.resolve()))

    def test_repeatable_config_overrides_preserve_values(self):
        config = 'experiment.tool_path=/tool chain/bin:/usr/bin'
        self.assertEqual(runner.main(['--config', config, '--config', 'runner.mode=local',
                                     '--project-dir', 'nested project',
                                     'github.com/example/tool', '--', 'tool']), 0)
        argv = json.loads(self.log.read_text())['args']
        self.assertEqual(argv[3:7], ['--config', config, '--config', 'runner.mode=local'])

    def test_explicit_target_label(self):
        self.assertEqual(self.run_adapter(['//cmd:tool']), 0)
        self.assertIn('//cmd:tool', json.loads(self.log.read_text())['args'])

    def test_prepare_hook_runs_and_failure_stops_buck(self):
        (self.project / 'prepare.sh').write_text('printf prepared > marker\nexit 17\n')
        self.assertEqual(self.run_adapter(), 17)
        self.assertEqual((self.project / 'marker').read_text(), 'prepared')
        self.assertFalse(self.log.exists())

    def test_runtime_exit_status(self):
        with patch.dict(os.environ, BUCK_TEST_EXIT='23'):
            self.assertEqual(self.run_adapter(), 23)

    def test_traversal_rejected_before_checkout(self):
        self.assertEqual(self.run_adapter(project='../escape'), 1)
        self.checkout.assert_not_called()

    def test_missing_workspace_is_clear_failure(self):
        (self.project / '.buckconfig').unlink()
        self.assertEqual(self.run_adapter(), 1)
        self.assertFalse(self.log.exists())

    def test_symlink_escape_is_rejected(self):
        (self.source / 'escape').symlink_to(self.root, target_is_directory=True)
        self.assertEqual(self.run_adapter(project='escape'), 1)
        self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()
