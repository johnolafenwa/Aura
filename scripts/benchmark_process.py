#!/usr/bin/env python3
"""Process-group ownership for Aura benchmark workload subprocesses."""

from __future__ import annotations

import contextlib
import os
import signal
import subprocess
import time
from typing import Iterator, List, Optional, Sequence


TERMINATE_TIMEOUT_SECONDS = 0.5
KILL_TIMEOUT_SECONDS = 3.0
POLL_INTERVAL_SECONDS = 0.01


class ProcessGroupCleanupError(RuntimeError):
    """A benchmark workload process group could not be fully cleaned up."""


def process_group_exists(process_group_id: int) -> bool:
    try:
        os.killpg(process_group_id, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # Existing-but-unsignalable is not clean.
        return True
    return True


def wait_for_process_group_exit(
    process: subprocess.Popen,
    process_group_id: int,
    timeout_seconds: float,
) -> bool:
    deadline = time.monotonic() + max(0.0, timeout_seconds)
    while True:
        # Reap the leader as soon as it exits. Its descendants may still keep
        # the independently-owned group alive.
        process.poll()
        if not process_group_exists(process_group_id):
            return True
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        time.sleep(min(POLL_INTERVAL_SECONDS, remaining))


def close_process_streams(process: subprocess.Popen) -> None:
    for stream in (process.stdin, process.stdout, process.stderr):
        if stream is not None and not stream.closed:
            stream.close()


def reap_process_group(
    process: subprocess.Popen,
    label: str,
    *,
    terminate_timeout_seconds: float = TERMINATE_TIMEOUT_SECONDS,
    kill_timeout_seconds: float = KILL_TIMEOUT_SECONDS,
) -> None:
    """Terminate, verify, and reap an independently-owned process group."""

    process_group_id = int(process.pid)
    failures: List[str] = []
    process.poll()

    if process_group_exists(process_group_id):
        try:
            os.killpg(process_group_id, signal.SIGTERM)
        except ProcessLookupError:
            pass
        except OSError as error:
            failures.append("SIGTERM failed: " + str(error))

        if not wait_for_process_group_exit(
            process,
            process_group_id,
            terminate_timeout_seconds,
        ):
            try:
                os.killpg(process_group_id, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError as error:
                failures.append("SIGKILL failed: " + str(error))

            if not wait_for_process_group_exit(
                process,
                process_group_id,
                kill_timeout_seconds,
            ):
                failures.append("process group is still alive after SIGKILL")

    # Limit damage even if group signalling or verification failed. This does
    # not turn the cleanup into success because descendants could remain.
    if process.poll() is None:
        try:
            process.kill()
        except ProcessLookupError:
            pass
        except OSError as error:
            failures.append("direct-child SIGKILL failed: " + str(error))
    try:
        process.wait(timeout=max(0.0, kill_timeout_seconds))
    except subprocess.TimeoutExpired:
        failures.append("process-group leader could not be reaped")
    except OSError as error:
        failures.append("process-group leader reap failed: " + str(error))
    finally:
        close_process_streams(process)

    if failures:
        raise ProcessGroupCleanupError(
            label + " process-group cleanup failed: " + "; ".join(failures)
        )


def launch_process_group(
    command: Sequence[str],
    **kwargs: object,
) -> subprocess.Popen:
    """Launch a workload in a new session whose group is owned by the caller."""

    if any(
        option in kwargs
        for option in ("start_new_session", "process_group", "preexec_fn", "shell")
    ):
        raise ValueError(
            "owned workload launch controls its own process-group settings"
        )
    return subprocess.Popen(
        list(command),
        start_new_session=True,
        shell=False,
        **kwargs,
    )


@contextlib.contextmanager
def owned_process_group(
    command: Sequence[str],
    label: str,
    **kwargs: object,
) -> Iterator[subprocess.Popen]:
    process = launch_process_group(command, **kwargs)
    try:
        yield process
    finally:
        reap_process_group(process, label)


def run_process_group(
    command: Sequence[str],
    label: str,
    *,
    timeout: Optional[float] = None,
    **kwargs: object,
) -> subprocess.CompletedProcess:
    """Run one workload while retaining descendant-cleanup ownership."""

    with owned_process_group(command, label, **kwargs) as process:
        stdout, stderr = process.communicate(timeout=timeout)
        return subprocess.CompletedProcess(
            list(command),
            process.returncode,
            stdout,
            stderr,
        )
