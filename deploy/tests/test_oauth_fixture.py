"""The fixture OAuth provider of a dev or ci cell (DEC-1176), on what the workspace chart renders.

With `oauthFixture.enabled` in a dev or ci tier, and only there, the chart renders the `oauth-fixture`
Deployment, its Service and its policy, and mounts the client Secret `cargo xtask cell up` makes,
since the chart makes none (DEC-822 item 4); the stub issuer's image runs as the
Alpaca provider alone, answering one public host; it reaches nothing, and only this workspace's
token exchange reaches it, whose token URL and `token-host` egress name it and nothing else. The API
alone names its authorize page, which the connect start hands the browser and the API never dials
(DEC-2633 item 3). Any other
tier refuses the setting, in the schema and in the chart, whether or not the stub's image is named.
What is expected is written out here, not read from the chart, so a chart that drifts fails.
"""

import copy
import json

from test_cell import (
    ALPACA_PAPER,
    ALPACA_SET,
    ALPACA_SET_VALUES,
    CELL,
    CHART,
    KUBE_API,
    STUB,
    TEMPLATES,
    containers,
    default_deny,
    every_image,
    gated,
    hardened,
    helm,
    mutate,
    pod_spec,
    policies,
    render_all,
    secrets_as_files,
    workloads,
)

ON = (
    "--set",
    f"images.stub-issuer={STUB}",
    "--set",
    "api.subjectLinks=subject-links",
    *ALPACA_SET,
    "--set",
    "oauthFixture.enabled=true",
)
TOKEN_URL = "http://oauth-fixture:8091/alpaca/oauth/token"
AUTHORIZE_URL = "http://oauth-fixture:8091/alpaca/oauth/authorize"
ACCOUNT_URL = "http://oauth-fixture:8091/alpaca/v2/account"
SECRET_FILE = "/run/oauth-fixture/client-secret"
MOUNTED_SECRET = "/run/secrets/oauth-fixture-client/client-secret"
ARGS = [
    "--alpaca-only",
    "--bind",
    "0.0.0.0:8091",
    "--public-host",
    "oauth-fixture:8091",
    "--alpaca-client-id",
    ALPACA_SET_VALUES["MANDATE_ALPACA_CLIENT_ID"],
    "--alpaca-client-secret-file",
    SECRET_FILE,
    "--alpaca-redirect-uri",
    ALPACA_SET_VALUES["MANDATE_ALPACA_REDIRECT_URI"],
]
FIXTURE = {"mandate.dev/workspace": "ws1", "mandate.dev/process": "oauth-fixture"}
EXCHANGE = {"mandate.dev/workspace": "ws1", "mandate.dev/process": "token-exchange"}
EXECUTOR = {"mandate.dev/workspace": "ws1", "mandate.dev/process": "executor"}
PORT = [{"protocol": "TCP", "port": 8091}]
OBJECTS = {
    ("Deployment", "oauth-fixture"),
    ("Service", "oauth-fixture"),
    ("NetworkPolicy", "oauth-fixture"),
}
BASE = (
    "-f",
    str(CELL / "values" / "k3d.yaml"),
    *every_image(),
    "--set-json",
    f'egress.kube-api=["{KUBE_API}"]',
)


def exchange_template(docs: list[dict]) -> dict:
    maps = [d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == TEMPLATES]
    return json.loads(maps[0]["data"]["token-exchange.json"])


def token_urls(docs: list[dict]) -> list[str]:
    return [
        e.get("value", "")
        for c in exchange_template(docs)["spec"]["containers"]
        for e in c.get("env", [])
        if e["name"] == "MANDATE_TOKEN_URL"
    ]


def authorize_urls(docs: list[dict]) -> list[tuple[str, str]]:
    """Each container that names `MANDATE_ALPACA_AUTHORIZE_URL`, as its process and the value: the
    page the API's connect start sends the browser to (DEC-2633 item 3)."""
    return [
        (w["metadata"]["labels"]["mandate.dev/process"], e.get("value", ""))
        for w in workloads(docs)
        for c in containers(w)
        for e in c.get("env", [])
        if e["name"] == "MANDATE_ALPACA_AUTHORIZE_URL"
    ]


def named_env(docs: list[dict], name: str) -> list[tuple[str, str]]:
    """Each container that names `name`, as its process and the value."""
    return [
        (w["metadata"]["labels"]["mandate.dev/process"], e.get("value", ""))
        for w in workloads(docs)
        for c in containers(w)
        for e in c.get("env", [])
        if e["name"] == name
    ]


def executor_reach(docs: list[dict]) -> list[dict]:
    """The executor's egress rules that name a pod or a CIDR beside the journal, the vault and DNS
    (DEC-1178 item 4)."""
    rules = policies(docs).get("egress-executor", {}).get("spec", {}).get("egress", [])
    known = [
        {"mandate.dev/workspace": "ws1", "mandate.dev/process": "vault"},
    ]
    return [
        r
        for r in rules
        if not any("namespaceSelector" in t for t in r["to"])
        and not any(t.get("podSelector", {}).get("matchLabels") in known for t in r["to"])
        and not any("cnpg.io/cluster" in t.get("podSelector", {}).get("matchLabels", {}) for t in r["to"])
    ]


def oauth_fixture(docs: list[dict], on: bool) -> list[str]:
    """What the chart renders for the fixture, on or off (DEC-1176 items 1 to 6)."""
    problems = []
    found = {
        (d["kind"], d["metadata"]["name"]) for d in docs if d["metadata"]["name"].startswith("oauth-fixture")
    }
    if found != (OBJECTS if on else set()):
        problems.append(f"the fixture's objects are {sorted(found)}")
    if token_urls(docs) != ([TOKEN_URL] if on else []):
        problems.append(f"the exchange's token URLs are {token_urls(docs)}")
    if authorize_urls(docs) != ([("api", AUTHORIZE_URL)] if on else []):
        problems.append(f"the authorize pages are {authorize_urls(docs)}")
    accounts = named_env(docs, "MANDATE_ALPACA_ACCOUNT_URL")
    if accounts != ([("executor", ACCOUNT_URL)] if on else []):
        problems.append(f"the account URLs are {accounts}")
    to_fixture_only = [{"to": [{"podSelector": {"matchLabels": FIXTURE}}], "ports": PORT}]
    if executor_reach(docs) != (to_fixture_only if on else []):
        problems.append(f"the executor reaches {executor_reach(docs)}")
    exchange = policies(docs).get("egress-token-exchange", {}).get("spec", {}).get("egress", [])
    to_fixture = [
        r for r in exchange if any(t.get("podSelector", {}).get("matchLabels") == FIXTURE for t in r["to"])
    ]
    if on:
        if to_fixture != [{"to": [{"podSelector": {"matchLabels": FIXTURE}}], "ports": PORT}]:
            problems.append(f"the exchange reaches the fixture by {to_fixture}")
        if any("ipBlock" in t for r in exchange for t in r["to"]):
            problems.append(f"the exchange reaches a CIDR beside the fixture: {exchange}")
    elif to_fixture:
        problems.append(f"the exchange reaches a fixture that does not run: {to_fixture}")
    if not on or problems:
        return problems
    deployment = next(
        d for d in docs if d["kind"] == "Deployment" and d["metadata"]["name"] == "oauth-fixture"
    )
    spec = pod_spec(deployment)
    main = spec["containers"]
    if deployment["spec"].get("replicas") != 1 or [c["name"] for c in main] != ["oauth-fixture"]:
        problems.append(
            f"the fixture runs {[c['name'] for c in main]} x {deployment['spec'].get('replicas')}"
        )
        return problems
    fixture = main[0]
    if fixture["image"] != STUB or fixture.get("args") != ARGS or "command" in fixture:
        problems.append(f"the fixture runs {fixture['image']} {fixture.get('command')} {fixture.get('args')}")
    tiers = [e.get("value") for e in fixture.get("env", []) if e["name"] == "MANDATE_DEPLOYMENT_TIER"]
    if len(tiers) != 1 or tiers[0] not in ("dev", "ci"):
        problems.append(f"the fixture's tier is {tiers}")
    volumes = {v["name"]: v for v in spec.get("volumes", [])}
    secret = volumes.get("client-secret", {}).get("secret")
    if secret != {
        "secretName": "oauth-fixture-client",
        "defaultMode": 288,
        "items": [{"key": "client-secret", "path": "client-secret"}],
    }:
        problems.append(f"the client secret is mounted as {secret}")
    if volumes.get("client", {}).get("emptyDir", {}).get("medium") != "Memory":
        problems.append(f"the client secret's copy is in {volumes.get('client')}")
    copies = [c for c in spec.get("initContainers", []) if c["name"] == "client-secret"]
    if [c.get("command") for c in copies] != [["install", "-m", "0600", MOUNTED_SECRET, SECRET_FILE]]:
        problems.append(f"the client secret is copied by {[c.get('command') for c in copies]}")
    mounted = {m["name"]: m for m in fixture.get("volumeMounts", [])}
    if "client-secret" in mounted or mounted.get("client", {}).get("readOnly") is not True:
        problems.append(f"the fixture mounts {sorted(mounted)}")
    service = next(d for d in docs if d["kind"] == "Service" and d["metadata"]["name"] == "oauth-fixture")
    if service["spec"] != {
        "type": "ClusterIP",
        "selector": FIXTURE,
        "ports": [{"name": "oauth", "port": 8091, "targetPort": 8091, "protocol": "TCP"}],
    }:
        problems.append(f"the fixture's Service is {service['spec']}")
    policy = policies(docs)["oauth-fixture"]["spec"]
    if policy != {
        "podSelector": {"matchLabels": FIXTURE},
        "policyTypes": ["Ingress", "Egress"],
        "ingress": [
            {
                "from": [
                    {"podSelector": {"matchLabels": EXCHANGE}},
                    {"podSelector": {"matchLabels": EXECUTOR}},
                ],
                "ports": PORT,
            }
        ],
        "egress": [],
    }:
        problems.append(f"the fixture's policy is {policy}")
    if secrets_as_files(docs):
        problems.extend(secrets_as_files(docs))
    return problems


def test_off_by_default():
    assert oauth_fixture(render_all(), on=False) == []
    assert (
        oauth_fixture(
            render_all("--set", f"images.stub-issuer={STUB}", "--set", "api.subjectLinks=s"), on=False
        )
        == []
    )


def test_in_dev_and_ci_the_fixture_runs_the_provider_alone_and_only_the_exchange_reaches_it():
    for tier in ("dev", "ci"):
        docs = render_all(*ON, "--set", f"deploymentTier={tier}")
        assert oauth_fixture(docs, on=True) == [], tier
        for check in (default_deny, gated, hardened, secrets_as_files):
            assert check(docs) == [], f"{tier} {check.__name__}"


def test_any_other_tier_refuses_it_in_the_schema_and_in_the_chart_with_or_without_the_image():
    with_image = ON
    without_image = ("--set", "oauthFixture.enabled=true")
    for tier in ("staging", "production"):
        base = (*BASE, "--set", f"deploymentTier={tier}")
        assert helm(CHART, *base).returncode == 0, f"{tier} renders without the fixture"
        for values in (with_image, without_image):
            for skip in ((), ("--skip-schema-validation",)):
                done = helm(CHART, *base, *values, *skip)
                assert done.returncode != 0, f"{tier} {values} {skip}"
                assert "oauthFixture" in done.stderr or "'dev', 'ci'" in done.stderr, done.stderr


def test_the_fixture_needs_its_image_its_client_and_no_token_host_cidr():
    for values in (
        ("--set", "oauthFixture.enabled=true", *ALPACA_SET),
        (
            "--set",
            "oauthFixture.enabled=true",
            "--set",
            f"images.stub-issuer={STUB}",
            "--set",
            "api.subjectLinks=s",
        ),
        (*ON, "--set-json", 'egress.token-host=["192.0.2.10/32"]'),
    ):
        done = helm(CHART, *BASE, *values)
        assert done.returncode != 0, values


def secret_volume(w: dict) -> dict:
    return next(v for v in pod_spec(w)["volumes"] if v["name"] == "client-secret")["secret"]


def fixture_container(w: dict) -> dict:
    return pod_spec(w)["containers"][0]


SEEDED = [
    (
        "NetworkPolicy",
        "oauth-fixture",
        lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}]}),
    ),
    ("NetworkPolicy", "oauth-fixture", lambda p: p["spec"].update(policyTypes=["Ingress"])),
    (
        "NetworkPolicy",
        "oauth-fixture",
        lambda p: p["spec"]["ingress"][0].update({"from": [{"podSelector": {}}]}),
    ),
    ("NetworkPolicy", "oauth-fixture", lambda p: p["spec"]["ingress"][0].pop("ports")),
    (
        "NetworkPolicy",
        "oauth-fixture",
        lambda p: p["spec"]["ingress"][0]["from"].append({"namespaceSelector": {}}),
    ),
    (
        "NetworkPolicy",
        "egress-token-exchange",
        lambda p: next(r for r in p["spec"]["egress"] if r.get("ports") == PORT)["to"].append(
            {"ipBlock": {"cidr": "192.0.2.10/32"}}
        ),
    ),
    ("Deployment", "oauth-fixture", lambda w: fixture_container(w)["args"].remove("--alpaca-only")),
    (
        "Deployment",
        "oauth-fixture",
        lambda w: fixture_container(w)["args"].__setitem__(4, "oauth-fixture:8092"),
    ),
    (
        "Deployment",
        "oauth-fixture",
        lambda w: fixture_container(w).update(image="mandate.local/api:0123456789ab"),
    ),
    ("Deployment", "oauth-fixture", lambda w: secret_volume(w).update(defaultMode=292)),
    (
        "Deployment",
        "oauth-fixture",
        lambda w: next(c for c in pod_spec(w)["initContainers"] if c["name"] == "client-secret")[
            "command"
        ].__setitem__(2, "0640"),
    ),
    (
        "Deployment",
        "oauth-fixture",
        lambda w: fixture_container(w)["volumeMounts"].append(
            {"name": "client-secret", "mountPath": "/run/secrets/oauth-fixture-client", "readOnly": True}
        ),
    ),
    (
        "Deployment",
        "oauth-fixture",
        lambda w: next(
            e for e in fixture_container(w)["env"] if e["name"] == "MANDATE_DEPLOYMENT_TIER"
        ).update(value="staging"),
    ),
    ("Service", "oauth-fixture", lambda s: s["spec"].update(type="NodePort")),
]


def test_each_seeded_bug_is_caught():
    docs = render_all(*ON)
    assert oauth_fixture(docs, on=True) == [], "the unseeded chart already fails"
    for kind, name, change in SEEDED:
        assert oauth_fixture(mutate(docs, name, kind, change), on=True), (
            f"missed a seeded bug in {kind} {name}"
        )
    for change in (
        lambda w: next(
            e
            for e in w["spec"]["template"]["spec"]["containers"][0]["env"]
            if e["name"] == "MANDATE_TOKEN_URL"
        ).update(value="https://api.alpaca.markets/oauth/token"),
        lambda w: w["spec"]["template"]["spec"]["containers"][0]["env"].remove(
            next(
                e
                for e in w["spec"]["template"]["spec"]["containers"][0]["env"]
                if e["name"] == "MANDATE_TOKEN_URL"
            )
        ),
    ):
        assert oauth_fixture(mutate(docs, "token-exchange", "Pod", change), on=True), "missed a token URL bug"

    def api_env(w):
        return w["spec"]["template"]["spec"]["containers"][0]["env"]

    def page(w):
        return next(e for e in api_env(w) if e["name"] == "MANDATE_ALPACA_AUTHORIZE_URL")

    for change in (
        lambda w: page(w).update(value="https://app.alpaca.markets/oauth/authorize"),
        lambda w: page(w).update(value="http://oauth-fixture:8091/alpaca/oauth/token"),
        lambda w: page(w).update(value=AUTHORIZE_URL + "?x=1"),
        lambda w: api_env(w).remove(page(w)),
        lambda w: api_env(w).append({"name": "MANDATE_ALPACA_AUTHORIZE_URL", "value": AUTHORIZE_URL}),
    ):
        changed = mutate(docs, "api", "Deployment", change)
        assert oauth_fixture(changed, on=True), "missed an authorize page bug"
    elsewhere = mutate(
        docs,
        "token-exchange",
        "Pod",
        lambda w: (
            pod_spec(w)["containers"][0]
            .setdefault("env", [])
            .append({"name": "MANDATE_ALPACA_AUTHORIZE_URL", "value": AUTHORIZE_URL})
        ),
    )
    assert oauth_fixture(elsewhere, on=True), "an authorize page on another process is caught"
    made = {
        "apiVersion": "v1",
        "kind": "Secret",
        "metadata": {"name": "oauth-fixture-client"},
        "type": "Opaque",
        "data": {"client-secret": "eA=="},
    }
    assert oauth_fixture([*docs, made], on=True), (
        "the client Secret made by the chart is caught (DEC-822 item 4)"
    )
    off = render_all()
    stray = copy.deepcopy(
        next(d for d in docs if d["kind"] == "Service" and d["metadata"]["name"] == "oauth-fixture")
    )
    assert oauth_fixture([*off, stray], on=False), "an object of a fixture that is off is caught"
    on_with_url_off = mutate(
        off,
        "token-exchange",
        "Pod",
        lambda w: (
            pod_spec(w)["containers"][0]
            .setdefault("env", [])
            .append({"name": "MANDATE_TOKEN_URL", "value": TOKEN_URL})
        ),
    )
    assert oauth_fixture(on_with_url_off, on=False), "a token URL with the fixture off is caught"
    page_with_fixture_off = mutate(
        off,
        "api",
        "Deployment",
        lambda w: w["spec"]["template"]["spec"]["containers"][0]["env"].append(
            {"name": "MANDATE_ALPACA_AUTHORIZE_URL", "value": AUTHORIZE_URL}
        ),
    )
    assert oauth_fixture(page_with_fixture_off, on=False), "a page with the fixture off is caught"

    def executor_env(w):
        return pod_spec(w)["containers"][0].setdefault("env", [])

    def account(w):
        return next(e for e in executor_env(w) if e["name"] == "MANDATE_ALPACA_ACCOUNT_URL")

    for change in (
        lambda w: account(w).update(value="https://paper-api.alpaca.markets/v2/account"),
        lambda w: account(w).update(value=TOKEN_URL),
        lambda w: account(w).update(value=ACCOUNT_URL + "?x=1"),
        lambda w: executor_env(w).remove(account(w)),
        lambda w: executor_env(w).append({"name": "MANDATE_ALPACA_ACCOUNT_URL", "value": ACCOUNT_URL}),
    ):
        assert oauth_fixture(mutate(docs, "executor", "Pod", change), on=True), "missed an account URL bug"
    on_exchange = mutate(
        docs,
        "token-exchange",
        "Pod",
        lambda w: (
            pod_spec(w)["containers"][0]
            .setdefault("env", [])
            .append({"name": "MANDATE_ALPACA_ACCOUNT_URL", "value": ACCOUNT_URL})
        ),
    )
    assert oauth_fixture(on_exchange, on=True), "an account URL on another process is caught"
    off_with_account = mutate(
        off,
        "executor",
        "Pod",
        lambda w: (
            pod_spec(w)["containers"][0]
            .setdefault("env", [])
            .append({"name": "MANDATE_ALPACA_ACCOUNT_URL", "value": ACCOUNT_URL})
        ),
    )
    assert oauth_fixture(off_with_account, on=False), "an account URL with the fixture off is caught"

    def fixture_rule(p):
        return next(r for r in p["spec"]["egress"] if r.get("ports") == PORT)

    for change in (
        lambda p: fixture_rule(p).update(ports=[{"protocol": "TCP", "port": 8090}]),
        lambda p: fixture_rule(p)["to"].append({"ipBlock": {"cidr": "0.0.0.0/0"}}),
        lambda p: p["spec"]["egress"].remove(fixture_rule(p)),
        lambda p: p["spec"]["egress"].append({"to": [{"ipBlock": {"cidr": "35.221.23.121/32"}}]}),
    ):
        assert oauth_fixture(mutate(docs, "egress-executor", "NetworkPolicy", change), on=True), (
            "missed an executor egress bug"
        )
    stray = mutate(
        off,
        "egress-executor",
        "NetworkPolicy",
        lambda p: p["spec"]["egress"].append(
            {"to": [{"podSelector": {"matchLabels": FIXTURE}}], "ports": PORT}
        ),
    )
    assert oauth_fixture(stray, on=False), "executor egress to a fixture that is off is caught"
    no_executor = mutate(
        docs,
        "oauth-fixture",
        "NetworkPolicy",
        lambda p: p["spec"]["ingress"][0]["from"].pop(),
    )
    assert oauth_fixture(no_executor, on=True), "a fixture that refuses the executor is caught"


def test_staging_and_production_name_no_account_url_and_their_executor_reaches_no_fixture():
    """DEC-1178 item 4: in staging, on Alpaca's paper host, and in production, the executor carries no
    account URL and its egress names nothing beside the journal, the vault and DNS; staging's one
    external host for it, `paper-api.alpaca.markets`, is its Cilium policy's (`test_gitops`)."""
    for name, docs in (
        ("staging", render_all(*ALPACA_PAPER)),
        ("production", render_all("--set", "deploymentTier=production")),
    ):
        assert named_env(docs, "MANDATE_ALPACA_ACCOUNT_URL") == [], name
        assert executor_reach(docs) == [], name


def test_a_token_url_is_admitted_by_its_one_fixture_value_alone():
    """`MANDATE_TOKEN_URL` names an address, so the secret-name check admits it, but only at the
    fixture's one URL: any other value still reads as a secret (DEC-1176 item 5)."""
    docs = render_all(*ON)
    assert secrets_as_files(docs) == []
    for value in (
        "https://api.alpaca.markets/oauth/token",
        "http://oauth-fixture:8091/alpaca/oauth/token?secret=x",
        "http://user:pass@oauth-fixture:8091/alpaca/oauth/token",
        "",
    ):
        changed = mutate(
            docs,
            "token-exchange",
            "Pod",
            lambda w, value=value: next(
                e for e in pod_spec(w)["containers"][0]["env"] if e["name"] == "MANDATE_TOKEN_URL"
            ).update(value=value),
        )
        assert secrets_as_files(changed), value
