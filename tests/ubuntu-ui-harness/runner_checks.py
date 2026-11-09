"""프로세스 생성 없이 종료 판정 회귀를 검사함."""

import unittest
from unittest.mock import Mock, patch
import run_case


class RunnerChecks(unittest.TestCase):
    def test_timeout_leader_exit_does_not_skip_descendants(self):
        process = Mock(returncode=0)
        owner = Mock()
        owner.refresh.return_value = {7: (99, 1, 7, "S")}
        with patch.object(run_case, "clean_owned", return_value=False) as cleanup:
            self.assertEqual(run_case.finish(process, owner, True), 125)
            cleanup.assert_called_once_with(process, owner)

    def test_timeout_cleaned_is_still_timeout(self):
        process = Mock(returncode=0)
        owner = Mock()
        owner.refresh.return_value = {}
        with patch.object(run_case, "clean_owned", return_value=True):
            self.assertEqual(run_case.finish(process, owner, True), 124)

    def test_normal_exit_with_descendants_cannot_pass(self):
        process = Mock(returncode=0)
        owner = Mock()
        owner.refresh.return_value = {7: (99, 1, 7, "S")}
        with patch.object(run_case, "clean_owned", return_value=True):
            self.assertEqual(run_case.finish(process, owner, False), 125)

    def test_pid_reuse_is_not_signalled(self):
        owner = object.__new__(run_case.OwnedProcesses)
        owner.leader = 7
        owner.identities = {7: 99}
        with patch.object(run_case, "snapshot", side_effect=[{7: (99, 1, 7, "S")}, {7: (100, 1, 8, "S")}]), \
                patch.object(run_case.os, "pidfd_open", return_value=42), \
                patch.object(run_case.os, "close"), \
                patch.object(run_case.signal, "pidfd_send_signal") as kill:
            owner.signal(15)
            kill.assert_not_called()


if __name__ == "__main__":
    unittest.main()
