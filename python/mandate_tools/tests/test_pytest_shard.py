"""The shard plugin's split (DEC-2646): the shards of any collection run every test exactly once."""

import random

import pytest

from mandate_tools.pytest_shard import kept, partition


@pytest.mark.parametrize(
    ("text", "parsed"),
    [("1/1", (1, 1)), ("1/4", (1, 4)), ("4/4", (4, 4)), ("12/40", (12, 40))],
)
def test_a_partition_reads_k_of_n(text, parsed):
    assert partition(text) == parsed


@pytest.mark.parametrize(
    "text",
    ["", "0/4", "5/4", "1/0", "01/4", "1/04", "1", "1/", "/4", "a/4", "1/4/2", " 1/4", "\u0661/\u0664"],
)
def test_anything_else_is_refused(text):
    with pytest.raises(ValueError):
        partition(text)


def test_the_shards_of_any_collection_run_every_test_exactly_once():
    rng = random.Random(2646)
    for _ in range(200):
        node_ids = [f"t{rng.randrange(10_000)}.py::test_{i}" for i in range(rng.randrange(0, 60))]
        n = rng.randrange(1, 8)
        shards = [kept(node_ids, k, n) for k in range(1, n + 1)]
        seen: list[str] = [node for shard in shards for node in shard]
        assert sorted(seen) == sorted(set(node_ids)), (n, node_ids)
        assert len(seen) == len(set(seen)), "no test runs in two shards"
        sizes = [len(shard) for shard in shards]
        assert not sizes or max(sizes) - min(sizes) <= 1, sizes


def test_the_split_ignores_collection_order():
    node_ids = [f"a.py::test_{i}" for i in range(10)]
    shuffled = list(reversed(node_ids))
    for k in range(1, 4):
        assert kept(node_ids, k, 3) == kept(shuffled, k, 3)
