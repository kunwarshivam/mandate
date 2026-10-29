"""A dev or ci cell's waiver of a first passkey's enrolment cool-off (DEC-2146 item 2), and a
staging cell's until its expiry (DEC-2150), on what the workspace chart renders.

With `api.stepUpCoolOffWaiver` in a dev or ci tier, or in staging with its expiry, the API
container, and no other container, carries `MANDATE_API_STEP_UP_COOL_OFF_WAIVER=true`; off, nothing
names it. Production refuses the setting, and staging refuses it without its expiry, in the schema
and in the chart, as the API refuses it at its start. What is expected is written out here, not read
from the chart, so a chart that drifts fails.
"""

import pytest
from test_cell import CHART, containers, helm, render_all, workloads
from test_oauth_fixture import BASE

NAME = "MANDATE_API_STEP_UP_COOL_OFF_WAIVER"
UNTIL_NAME = "MANDATE_API_STEP_UP_COOL_OFF_WAIVER_UNTIL"
ON = ("--set", "api.stepUpCoolOffWaiver=true")
EXPIRY = "2026-11-01T12:00:00Z"
UNTIL = ("--set-string", f"api.stepUpCoolOffWaiverUntil={EXPIRY}")

#: DEC-2150's chart render, which the implementation lands; until then these fail as expected.
PENDING = pytest.mark.xfail(strict=True, reason="pending E9-4: DEC-2150's render of the staging waiver")


def named(docs: list[dict], name: str = NAME) -> list[tuple[str, str, str]]:
    """Each container that names `name`: its process, its container, and the value."""
    return [
        (w["metadata"]["labels"]["mandate.dev/process"], c["name"], e.get("value", ""))
        for w in workloads(docs)
        for c in containers(w)
        for e in c.get("env", [])
        if e["name"] == name
    ]


def test_off_by_default_in_every_tier():
    for tier in ("dev", "ci", "staging", "production"):
        done = helm(CHART, *BASE, "--set", f"deploymentTier={tier}")
        assert done.returncode == 0, f"{tier}: {done.stderr}"
        assert NAME not in done.stdout, tier
    assert named(render_all()) == []


def test_in_dev_and_ci_only_the_api_container_carries_it():
    for tier in ("dev", "ci"):
        docs = render_all(*ON, "--set", f"deploymentTier={tier}")
        assert named(docs) == [("api", "api", "true")], tier


def test_any_other_tier_refuses_it_in_the_schema_and_in_the_chart():
    for tier in ("staging", "production"):
        base = (*BASE, "--set", f"deploymentTier={tier}")
        for skip in ((), ("--skip-schema-validation",)):
            done = helm(CHART, *base, *ON, *skip)
            assert done.returncode != 0, f"{tier} {skip}"
            assert "stepUpCoolOffWaiver" in done.stderr or "'dev', 'ci'" in done.stderr, done.stderr
            assert NAME not in done.stdout, f"{tier} {skip}"


def test_the_value_is_a_boolean_in_the_schema():
    for value in ('"true"', "1", "null"):
        done = helm(
            CHART, *BASE, "--set", "deploymentTier=ci", "--set-json", f"api.stepUpCoolOffWaiver={value}"
        )
        assert done.returncode != 0, value


@PENDING
def test_in_staging_with_an_expiry_only_the_api_container_carries_both():
    """DEC-2150 item 6: a staging cell renders the waiver and its expiry on the API alone."""
    docs = render_all(*ON, *UNTIL, "--set", "deploymentTier=staging")
    assert named(docs) == [("api", "api", "true")]
    assert named(docs, UNTIL_NAME) == [("api", "api", EXPIRY)]


def test_staging_without_an_expiry_is_refused_in_the_schema_and_in_the_chart():
    """DEC-2150 item 6: staging's waiver needs its expiry, at render too."""
    base = (*BASE, "--set", "deploymentTier=staging")
    for skip in ((), ("--skip-schema-validation",)):
        done = helm(CHART, *base, *ON, *skip)
        assert done.returncode != 0, skip
        assert NAME not in done.stdout, skip


def test_production_refuses_it_with_an_expiry_in_the_schema_and_in_the_chart():
    """DEC-2150 item 1: production never waives, with an expiry or without."""
    base = (*BASE, "--set", "deploymentTier=production")
    for skip in ((), ("--skip-schema-validation",)):
        done = helm(CHART, *base, *ON, *UNTIL, *skip)
        assert done.returncode != 0, skip
        assert NAME not in done.stdout, skip
        assert UNTIL_NAME not in done.stdout, skip


@PENDING
def test_an_expiry_without_the_waiver_is_refused_in_the_schema_and_in_the_chart():
    """DEC-2150 item 6: a stray expiry is refused, never rendered alone."""
    for tier in ("dev", "ci", "staging"):
        base = (*BASE, "--set", f"deploymentTier={tier}")
        for skip in ((), ("--skip-schema-validation",)):
            done = helm(CHART, *base, *UNTIL, *skip)
            assert done.returncode != 0, f"{tier} {skip}"
            assert UNTIL_NAME not in done.stdout, f"{tier} {skip}"


@PENDING
def test_a_test_cell_renders_an_expiry_it_is_given():
    """DEC-2150 item 2: dev and ci need no expiry, and render one they are given."""
    for tier in ("dev", "ci"):
        docs = render_all(*ON, *UNTIL, "--set", f"deploymentTier={tier}")
        assert named(docs, UNTIL_NAME) == [("api", "api", EXPIRY)], tier


def test_the_expiry_takes_one_form_in_the_schema():
    """DEC-2150 item 2: the expiry is YYYY-MM-DDTHH:MM:SSZ, nothing else."""
    for value in ('"2026-11-01"', '"2026-11-01T12:00:00.000Z"', '"2026-11-01T12:00:00+00:00"', "1"):
        done = helm(
            CHART,
            *BASE,
            "--set",
            "deploymentTier=staging",
            *ON,
            "--set-json",
            f"api.stepUpCoolOffWaiverUntil={value}",
        )
        assert done.returncode != 0, value
