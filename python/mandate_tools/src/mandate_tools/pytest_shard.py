"""A pytest plugin that runs one shard of the collected tests (DEC-464, DEC-2646).

``cargo xtask ci test-python`` loads it with ``-p mandate_tools.pytest_shard`` when
``MANDATE_TEST_PARTITION`` is ``K/N``, counted from 1, as CI's ``fast-python`` matrix sets it. Every
shard collects the same tests, sorts their node ids, and keeps those at positions ``K-1``,
``K-1+N``, ``K-1+2N`` and so on, so the ``N`` shards together run every test exactly once. The split
is by test, not by file, because the slowest tests come two to a file.

A shard that keeps no test fails: pytest reports no tests ran, exit status 5.
"""

import os

PARTITION_ENV = "MANDATE_TEST_PARTITION"


def partition(text: str) -> tuple[int, int]:
    """``K/N`` as ``(K, N)``: plain decimals, no leading zero, ``1 <= K <= N``."""
    k, sep, n = text.partition("/")

    def plain(part: str) -> bool:
        return part.isascii() and part.isdigit() and not part.startswith("0")

    if not sep or not plain(k) or not plain(n) or int(k) > int(n):
        raise ValueError(f"{PARTITION_ENV} must be K/N with 1 <= K <= N, counted from 1; got {text!r}")
    return int(k), int(n)


def kept(node_ids: list[str], k: int, n: int) -> set[str]:
    """The node ids shard ``k`` of ``n`` runs: every ``n``-th of them in sorted order, from ``k-1``."""
    return {node for index, node in enumerate(sorted(node_ids)) if index % n == k - 1}


def pytest_collection_modifyitems(session, config, items) -> None:
    text = os.environ.get(PARTITION_ENV, "")
    if not text:
        raise ValueError(f"mandate_tools.pytest_shard is loaded but {PARTITION_ENV} is unset")
    k, n = partition(text)
    keep = kept([item.nodeid for item in items], k, n)
    deselected = [item for item in items if item.nodeid not in keep]
    items[:] = [item for item in items if item.nodeid in keep]
    config.hook.pytest_deselected(items=deselected)
