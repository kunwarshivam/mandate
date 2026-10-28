#!/usr/bin/env python3

import os
from pathlib import Path
import stat
import subprocess
import tempfile
import textwrap
import unittest


MODULE_ROOT = Path(__file__).resolve().parent.parent
REPOSITORY_ROOT = MODULE_ROOT.parents[3]


def rendered_manager() -> str:
    source = (MODULE_ROOT / "main.tf").read_text()
    marker = "  manager = <<-MANAGER\n"
    start = source.index(marker) + len(marker)
    end = source.index("\n  MANAGER", start)
    lines = source[start:end].splitlines()
    rendered = "\n".join(
        line[4:] if line.startswith("    ") else line for line in lines
    )
    return rendered.replace("$${", "${") + "\n"


def executable(path: Path, content: str) -> None:
    path.write_text(textwrap.dedent(content).lstrip())
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


class ManagerHarness:
    def __init__(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.bin = self.root / "bin"
        self.config = self.root / "config"
        self.state = self.root / "state"
        self.log = self.root / "calls.log"
        self.bin.mkdir()
        self.config.mkdir()
        self.state.mkdir()
        self.log.touch()
        (self.config / "env").write_text(
            "\n".join(
                [
                    "APP_CLIENT_ID=test-client",
                    "INSTALLATION_ID=123",
                    "REPOSITORY=kunwarshivam/mandate",
                    "REPOSITORY_NAME=mandate",
                    "LABEL=mandate-ci-do",
                ]
            )
            + "\n"
        )
        (self.config / "app.pem").write_text("fixture\n")
        self.manager = self.root / "manager"
        executable(self.manager, rendered_manager())
        executable(
            self.bin / "date",
            """
            #!/bin/bash
            if [ "$1" = +%s ]; then
              printf '200000\n'
            else
              printf '200000\n'
            fi
            """,
        )
        executable(
            self.bin / "openssl",
            """
            #!/bin/bash
            if [ "$1" = base64 ]; then
              base64
            else
              cat > /dev/null
              printf 'signature'
            fi
            """,
        )
        executable(
            self.bin / "sleep",
            """
            #!/bin/bash
            printf 'sleep %s\n' "$1" >> "$MANDATE_TEST_LOG"
            """,
        )
        executable(
            self.bin / "docker",
            """
            #!/bin/bash
            printf 'docker %s\n' "$*" >> "$MANDATE_TEST_LOG"
            case "$1 $2" in
              "image inspect")
                if printf '%s\n' "$*" | grep -q Created; then
                  printf '2026-01-01T00:00:00Z\n'
                else
                  printf 'sha256:test-image\n'
                fi
                ;;
              "image prune" | "container prune" | "rm --force" | "stop --time")
                ;;
              "ps --quiet")
                ;;
              "wait mandate-ci-job")
                ;;
              "build --no-cache")
                exit "${FAKE_BUILD_STATUS:-0}"
                ;;
              "run --rm")
                if [ "${*: -1}" = mandate-runner-preflight ]; then
                  exit "${FAKE_PREFLIGHT_STATUS:-0}"
                fi
                /bin/sleep 0.2
                exit "${FAKE_RUNNER_STATUS:-0}"
                ;;
              *)
                exit 64
                ;;
            esac
            """,
        )
        executable(
            self.bin / "curl",
            """
            #!/bin/bash
            printf 'curl %s\n' "$*" >> "$MANDATE_TEST_LOG"
            url=
            for argument in "$@"; do
              case "$argument" in
                https://*) url=$argument ;;
              esac
            done
            case "$url" in
              */access_tokens)
                printf '{"token":"fixture-token"}\n'
                ;;
              */fork-pr-contributor-approval)
                printf '{"approval_policy":"all_external_contributors"}\n'
                ;;
              */generate-jitconfig)
                printf 'generate-jitconfig\n' >> "$MANDATE_TEST_LOG"
                printf '{"runner":{"id":4242},"encoded_jit_config":"fixture-config"}\n'
                ;;
              *"/actions/runners?per_page=100")
                printf '{"runners":[{"id":4242,"name":"mandate-ci-runner-test","status":"offline"}]}\n'
                ;;
              */actions/runners/*)
                if printf '%s\n' "$*" | grep -q -- '-X DELETE'; then
                  printf 'delete-runner %s\n' "${url##*/}" >> "$MANDATE_TEST_LOG"
                else
                  printf '{"id":4242,"status":"%s"}\n' "${FAKE_RUNNER_STATE:-online}"
                fi
                ;;
              */installation/token)
                ;;
              *)
                exit 64
                ;;
            esac
            """,
        )

    def close(self) -> None:
        self.temporary.cleanup()

    def run(
        self,
        *,
        preflight_status: int = 0,
        runner_status: int = 0,
        runner_state: str = "online",
    ) -> subprocess.CompletedProcess[str]:
        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{self.bin}:{environment['PATH']}",
                "MANDATE_RUNNER_CONFIG_DIR": str(self.config),
                "MANDATE_RUNNER_STATE_DIR": str(self.state),
                "MANDATE_RUNNER_BOOT_ID": "fixture-boot",
                "MANDATE_TEST_LOG": str(self.log),
                "FAKE_PREFLIGHT_STATUS": str(preflight_status),
                "FAKE_RUNNER_STATUS": str(runner_status),
                "FAKE_RUNNER_STATE": runner_state,
            }
        )
        return subprocess.run(
            [str(self.manager)],
            check=False,
            capture_output=True,
            env=environment,
            text=True,
            timeout=10,
        )

    def calls(self) -> str:
        return self.log.read_text()

    def clear_calls(self) -> None:
        self.log.write_text("")


class RunnerManagerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.harness = ManagerHarness()

    def tearDown(self) -> None:
        self.harness.close()

    def test_failed_preflight_creates_no_jit_registration(self) -> None:
        result = self.harness.run(preflight_status=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("generate-jitconfig", self.harness.calls())
        self.assertIn("sleep 30", self.harness.calls())

    def test_failed_listener_deletes_registration_before_backoff(self) -> None:
        result = self.harness.run(runner_status=1)
        self.assertNotEqual(result.returncode, 0)
        calls = self.harness.calls()
        self.assertLess(calls.index("delete-runner 4242"), calls.index("sleep 30"))
        self.assertFalse((self.harness.state / "current-runner-id").exists())

    def test_listener_that_never_becomes_online_is_deleted(self) -> None:
        result = self.harness.run(runner_state="offline")
        self.assertNotEqual(result.returncode, 0)
        calls = self.harness.calls()
        self.assertLess(calls.index("delete-runner 4242"), calls.index("sleep 30"))
        self.assertFalse((self.harness.state / "current-runner-id").exists())

    def test_prior_registration_is_deleted_before_a_new_jit_registration(self) -> None:
        (self.harness.state / "current-runner-id").write_text("313")
        result = self.harness.run()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.harness.calls()
        self.assertLess(
            calls.index("delete-runner 313"), calls.index("generate-jitconfig")
        )

    def test_failure_backoff_is_bounded(self) -> None:
        delays = []
        for _ in range(7):
            self.harness.clear_calls()
            result = self.harness.run(preflight_status=1)
            self.assertNotEqual(result.returncode, 0)
            sleeps = [
                int(line.removeprefix("sleep "))
                for line in self.harness.calls().splitlines()
                if line.startswith("sleep ")
            ]
            delays.append(sleeps[-1])
        self.assertEqual(delays, [30, 60, 120, 240, 480, 900, 900])

    def test_success_resets_failure_count(self) -> None:
        (self.harness.state / "failure-count").write_text("5")
        result = self.harness.run()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.harness.state / "failure-count").read_text(), "0")

    def test_environment_root_pins_the_stable_runner(self) -> None:
        root = (
            REPOSITORY_ROOT / "deploy/tofu/environments/ci-runners/main.tf"
        ).read_text()
        self.assertIn('runner_version             = "2.338.2"', root)
        self.assertIn(
            'runner_sha256              = "baebf204307bac6193480852732f0aba2ae97b6c5597c244f4d5558390147c40"',
            root,
        )


if __name__ == "__main__":
    unittest.main()
