"""A dev or ci cell's waiver of a first passkey's enrolment cool-off (DEC-2146 item 2), on what the
workspace chart renders.

With `api.stepUpCoolOffWaiver` in a dev or ci tier, and only there, the API container, and no other
container, carries `MANDATE_API_STEP_UP_COOL_OFF_WAIVER=true`; off, nothing names it. Any other
tier refuses the setting, in the schema and in the chart, as the API refuses it at its start. What
is expected is written out here, not read from the chart, so a chart that drifts fails.
"""

from test_cell import CHART, helm, render_all, workloads, containers
from test_oauth_fixture import BASE

NAME = "MANDATE_API_STEP_UP_COOL_OFF_WAIVER"
ON = ("--set", "api.stepUpCoolOffWaiver=true")


def named(docs: list[dict]) -> list[tuple[str, str, str]]:
    """Each container that names the waiver: its process, its container, and the value."""
    return [
        (w["metadata"]["labels"]["mandate.dev/process"], c["name"], e.get("value", ""))
        for w in workloads(docs)
        for c in containers(w)
        for e in c.get("env", [])
        if e["name"] == NAME
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
        done = helm(CHART, *BASE, "--set", "deploymentTier=ci", "--set-json", f"api.stepUpCoolOffWaiver={value}")
        assert done.returncode != 0, value
