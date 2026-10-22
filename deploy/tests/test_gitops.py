"""Invariants of the hosted clusters' GitOps overlays (E21-58, DEC-2015, DEC-2017), with no credential.

Each overlay is built offline with `flux build --dry-run`, exactly as
kustomize-controller builds it. Every HelmRelease's values are merged as helm-controller merges them,
and each of the repository's own charts is rendered from deploy/cell/charts/, the source
`cargo xtask release publish-charts` packages. As in test_cell.py, each property is one function
returning the problems it finds, asserted empty on the real overlays and shown non-empty on a seeded
bug. The expected pins and the signer are written out here from DEC-2015, not read from the overlays,
so an overlay that drifts fails. The overlays checked are every environment's under
deploy/gitops/clusters/ and a fixture of staging's under deploy/tests/fixtures/gitops/, which also
takes the seeded bugs.
"""

import copy
import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest
import yaml
from test_cell import api_vault

ROOT = Path(__file__).resolve().parents[2]
GITOPS = ROOT / "deploy" / "gitops"
CHARTS = ROOT / "deploy" / "cell" / "charts"
ENVIRONMENTS = ("staging", "production")
FIXTURE = ROOT / "deploy" / "tests" / "fixtures" / "gitops" / "clusters" / "staging"
# Each overlay checked, as (environment, directory): the fixture, and every real one that exists.
OVERLAYS = [("staging", FIXTURE)] + [
    (environment, GITOPS / "clusters" / environment)
    for environment in ENVIRONMENTS
    if (GITOPS / "clusters" / environment).is_dir()
]
REPOSITORY = "https://github.com/kunwarshivam/mandate"
# The repository's own charts, by OCIRepository name: the directory publish-charts packages.
OWN_CHARTS = {
    "mandate-backup-plugin-tls": "backup-plugin-tls",
    "mandate-journal-db": "journal-db",
    "mandate-workspace": "workspace",
    "mandate-vault-server": "vault",
}
# The charts staging alone runs: the vault, whose production unseal is not decided (DEC-2021 item 6,
# DEC-2051).
STAGING_ONLY = {"mandate-vault-server"}
VAULT_VALUES = ROOT / "deploy" / "cell" / "values" / "vault-staging.yaml"
OWN_CHART_URL = "oci://ghcr.io/kunwarshivam/mandate/charts/"
# The upstream charts and the digests whose chart layers are xtask/src/cell.rs's pinned tarballs.
UPSTREAM_CHARTS = {
    "cloudnative-pg": (
        "oci://ghcr.io/cloudnative-pg/charts/cloudnative-pg",
        "sha256:3245fa051bb21d0dd9246272f57a2760b4234817a164a3167d2a1e7fb9f63fd9",
    ),
    "plugin-barman-cloud": (
        "oci://ghcr.io/cloudnative-pg/charts/plugin-barman-cloud",
        "sha256:b48511269e2c75603ee2f06f6c5444e8154542103a3c3b9d692344725bac37b7",
    ),
    "policy-controller": (
        "oci://ghcr.io/sigstore/helm-charts/policy-controller",
        "sha256:9ff3ca6ae4a0155de4dd667f8b760e56e1bd5c8233a0321ad870a654ce022f2f",
    ),
}
CHART_LAYER = {"mediaType": "application/vnd.cncf.helm.chart.content.v1.tar+gzip", "operation": "copy"}
# The one identity whose keyless signature Flux accepts on the repository's charts: release.yml on main.
SIGNER = {
    "issuer": r"^https://token\.actions\.githubusercontent\.com$",
    "subject": r"^https://github\.com/kunwarshivam/mandate/\.github/workflows/release\.yml@refs/heads/main$",
}
# The admission policy (DEC-2016 item 1): this repository's images, release.yml's identity on main.
POLICIES = GITOPS / "policies"
ADMITTED_IMAGES = [{"glob": "ghcr.io/kunwarshivam/mandate/**"}]
ADMISSION_AUTHORITY = {
    "keyless": {
        "url": "https://fulcio.sigstore.dev",
        "identities": [
            {
                "issuer": "https://token.actions.githubusercontent.com",
                "subjectRegExp": SIGNER["subject"],
            }
        ],
    },
    "ctlog": {"url": "https://rekor.sigstore.dev"},
}
ADMISSION_LABEL = {"policy.sigstore.dev/include": "true"}
# The journal's archive: R2's S3 endpoint for one account, reached only at R2's two addresses on 443,
# resolved 2026-10-18 for the default jurisdiction (DEC-2017 items 1 and 2).
R2_ENDPOINT = re.compile(r"^https://[0-9a-f]{32}\.r2\.cloudflarestorage\.com$")
R2_EGRESS = {"cidrs": ["172.64.190.1/32", "172.64.66.1/32"], "port": 443}
# The ConfigMaps the bootstrap writes, not git (DEC-2017 item 6), each with a stand-in value of the
# shape the bootstrap writes: the archive's endpoint names the founder's Cloudflare account.
STAND_IN_ACCOUNT = "0123456789abcdef0123456789abcdef"
BOOTSTRAP_VALUES = {
    "journal-archive-endpoint": {
        "archive": {"endpointURL": f"https://{STAND_IN_ACCOUNT}.r2.cloudflarestorage.com"}
    },
}
# The tiers the stub issuer may run in (DEC-2005), which no hosted environment may take.
STUB_TIERS = {"dev", "ci"}
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
PINNED = re.compile(r"@sha256:[0-9a-f]{64}$")
KUBE_API = ["--set-json", 'kubeApi.cidrs=["10.245.0.1/32"]', "--set-json", "kubeApi.ports=[443]"]
PRUNE_DISABLED = {"kustomize.toolkit.fluxcd.io/prune": "disabled"}

# The kinds kubeconform has no schema for: CloudNativePG's and the Barman Cloud plugin's, skipped by
# kind as `cargo xtask ci cell` skips them, never by a missing-schema wildcard.
NO_SCHEMA = "Cluster,ObjectStore,ScheduledBackup"

# Set in CI's fast-python job, which installs all three tools (DEC-2017 item 3); other jobs that run
# deploy/tests without flux, such as the cell's, skip this file.
REQUIRED = os.environ.get("MANDATE_GITOPS_REQUIRED")
for tool in ("helm", "flux", "kubeconform"):
    if shutil.which(tool) is None:
        if REQUIRED:
            raise RuntimeError(f"MANDATE_GITOPS_REQUIRED is set but {tool} is not installed")
        pytest.skip(f"{tool} is not installed (.cursor/install.sh)", allow_module_level=True)


def build(environment: str, overlay: Path, tmp: Path) -> list[dict]:
    """The overlay as kustomize-controller builds it, from its own sync.yaml's Kustomization."""
    sync = [d for d in yaml.safe_load_all((overlay / "sync.yaml").read_text()) if d]
    kustomization = [d for d in sync if d["kind"] == "Kustomization"]
    assert len(kustomization) == 1, f"{environment}: sync.yaml needs exactly one Kustomization"
    spec_file = tmp / f"{environment}-kustomization.yaml"
    spec_file.write_text(yaml.safe_dump(kustomization[0]))
    done = subprocess.run(
        [
            *("flux", "build", "kustomization", "cluster", "--path", str(overlay)),
            *("--kustomization-file", str(spec_file), "--dry-run"),
        ],
        capture_output=True,
        text=True,
        check=False,
        cwd=ROOT,
    )
    assert done.returncode == 0, done.stderr
    return [d for d in yaml.safe_load_all(done.stdout) if d]


def of_kind(docs: list[dict], kind: str) -> dict[str, dict]:
    return {d["metadata"]["name"]: d for d in docs if d["kind"] == kind}


def merge(base: dict, over: dict) -> dict:
    """Helm's coalescing of values: maps merge key by key, anything else is replaced."""
    out = copy.deepcopy(base)
    for key, value in over.items():
        if isinstance(value, dict) and isinstance(out.get(key), dict):
            out[key] = merge(out[key], value)
        else:
            out[key] = copy.deepcopy(value)
    return out


def release_values(docs: list[dict], release: dict) -> dict:
    """A HelmRelease's values as helm-controller merges them: each valuesFrom in order, then values."""
    maps = of_kind(docs, "ConfigMap")
    values: dict = {}
    for ref in release["spec"].get("valuesFrom", []):
        assert ref["kind"] == "ConfigMap", ref
        if ref["name"] in BOOTSTRAP_VALUES:
            assert ref["name"] not in maps, f"{ref['name']} is the bootstrap's, not git's"
            values = merge(values, BOOTSTRAP_VALUES[ref["name"]])
            continue
        source = maps.get(ref["name"])
        assert source is not None, f"valuesFrom names a ConfigMap the overlay lacks: {ref['name']}"
        values = merge(values, yaml.safe_load(source["data"][ref.get("valuesKey", "values.yaml")]) or {})
    return merge(values, release["spec"].get("values") or {})


def render(release: dict, values: dict, tmp: Path) -> list[dict]:
    """One of the repository's own charts, rendered from its directory with the release's values."""
    chart = OWN_CHARTS[release["spec"]["chartRef"]["name"]]
    values_file = tmp / f"{release['metadata']['name']}-values.yaml"
    values_file.write_text(yaml.safe_dump(values))
    extra = KUBE_API if chart in ("journal-db", "vault") else []
    done = subprocess.run(
        [
            *("helm", "template", release["spec"]["releaseName"], str(CHARTS / chart)),
            *("--namespace", release["spec"]["targetNamespace"], "-f", str(values_file), *extra),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode == 0, done.stderr
    return [d for d in yaml.safe_load_all(done.stdout) if d]


def sources_pinned(docs: list[dict], environment: str = "staging") -> list[str]:
    """Every chart comes by digest; the repository's own only once release.yml's signature on main
    verifies; the upstream charts are exactly the pinned ones; and only staging runs the vault."""
    problems = []
    sources = of_kind(docs, "OCIRepository")
    for name, source in sources.items():
        spec = source["spec"]
        if set(spec.get("ref", {})) != {"digest"} or not DIGEST.match(spec["ref"]["digest"]):
            problems.append(f"{name}: not pinned by digest alone: {spec.get('ref')}")
        if spec.get("layerSelector") != CHART_LAYER:
            problems.append(f"{name}: does not select the chart layer")
        if name in STAGING_ONLY and environment != "staging":
            problems.append(f"{name}: runs in staging alone (DEC-2051)")
        if name in OWN_CHARTS:
            if spec.get("url") != OWN_CHART_URL + name:
                problems.append(f"{name}: url {spec.get('url')}")
            verify = spec.get("verify") or {}
            if verify.get("provider") != "cosign" or verify.get("matchOIDCIdentity") != [SIGNER]:
                problems.append(f"{name}: verifies {verify}, not release.yml's keyless signature on main")
            if "secretRef" in verify:
                problems.append(f"{name}: verifies against a key, not the keyless identity")
        elif name in UPSTREAM_CHARTS:
            if (spec.get("url"), spec.get("ref", {}).get("digest")) != UPSTREAM_CHARTS[name]:
                problems.append(f"{name}: not the pinned upstream chart")
        else:
            problems.append(f"{name}: an OCIRepository the hosted clusters do not run")
    expected = set(OWN_CHARTS) | set(UPSTREAM_CHARTS)
    if environment != "staging":
        expected -= STAGING_ONLY
    for name in expected:
        if name not in sources:
            problems.append(f"{name}: missing")
    return problems


def releases_resolve(docs: list[dict]) -> list[str]:
    """Every HelmRelease installs a chart by digest: a chartRef to one of the overlay's
    OCIRepositories, never a chart read from git, whose artifacts drop the charts' symlinks."""
    problems = []
    sources = of_kind(docs, "OCIRepository")
    for name, release in of_kind(docs, "HelmRelease").items():
        spec = release["spec"]
        ref = spec.get("chartRef") or {}
        if "chart" in spec:
            problems.append(f"{name}: a chart from a source other than its OCIRepository")
        if ref.get("kind") != "OCIRepository" or ref.get("name") not in sources:
            problems.append(f"{name}: chartRef {ref} names no OCIRepository of the overlay")
    return problems


def sync_read_only(docs: list[dict], overlay: Path) -> list[str]:
    """The cluster reads this public repository anonymously from main and only its own directory."""
    problems = []
    git = of_kind(docs, "GitRepository")
    if set(git) != {"mandate"}:
        problems.append(f"git sources {sorted(git)}")
    for source in git.values():
        spec = source["spec"]
        if spec.get("url") != REPOSITORY or spec.get("ref") != {"branch": "main"}:
            problems.append(f"git source {spec.get('url')} at {spec.get('ref')}")
        if "secretRef" in spec:
            problems.append("the git source holds a credential")
    kustomizations = of_kind(docs, "Kustomization")
    paths = {"cluster": f"./{overlay.relative_to(ROOT)}", "policies": "./deploy/gitops/policies"}
    if set(kustomizations) != set(paths):
        problems.append(f"Kustomizations {sorted(kustomizations)}")
    for name, kustomization in kustomizations.items():
        spec = kustomization["spec"]
        if spec.get("path") != paths.get(name):
            problems.append(f"{name} reconciles {spec.get('path')}")
        if spec.get("sourceRef") != {"kind": "GitRepository", "name": "mandate"}:
            problems.append(f"reconciles from {spec.get('sourceRef')}")
    return problems


def endpoint_from_bootstrap(docs: list[dict]) -> list[str]:
    """The journal's archive endpoint comes only from the bootstrap's ConfigMap, never from git,
    which holds no account (DEC-2017 item 6)."""
    journal = of_kind(docs, "HelmRelease").get("journal") or {"spec": {}}
    spec = journal["spec"]
    problems = []
    refs = [ref for ref in spec.get("valuesFrom", []) if ref.get("name") == "journal-archive-endpoint"]
    if len(refs) != 1:
        problems.append("journal: does not read the bootstrap's journal-archive-endpoint once")
    if any(ref.get("optional") for ref in refs):
        problems.append("journal: installs without the bootstrap's endpoint, so with none")
    if "endpointURL" in ((spec.get("values") or {}).get("archive") or {}):
        problems.append("journal: git sets the archive endpoint")
    if "journal-archive-endpoint" in of_kind(docs, "ConfigMap"):
        problems.append("journal-archive-endpoint is in git, so Flux would overwrite the bootstrap's")
    return problems


def vault_release(docs: list[dict], environment: str = "staging") -> list[str]:
    """Staging installs the vault with vault-staging.yaml unchanged, beside the workspace, never
    waiting on a vault that stays sealed until the founder initializes it, and never pruned;
    production installs none (DEC-2051)."""
    release = of_kind(docs, "HelmRelease").get("vault")
    if environment != "staging":
        return [] if release is None else ["vault: runs in staging alone (DEC-2051)"]
    if release is None:
        return ["vault: missing"]
    spec = release["spec"]
    problems = []
    if spec.get("chartRef") != {"kind": "OCIRepository", "name": "mandate-vault-server"}:
        problems.append(f"vault: installs {spec.get('chartRef')}")
    if (spec.get("targetNamespace"), spec.get("storageNamespace")) != ("ws-ws1", "ws-ws1"):
        problems.append("vault: not beside the workspace in ws-ws1")
    for phase in ("install", "upgrade"):
        if (spec.get(phase) or {}).get("disableWait") is not True:
            problems.append(f"vault: {phase} waits on a vault the founder has not initialized")
    if spec.get("values") or spec.get("valuesFrom") != [
        {"kind": "ConfigMap", "name": "vault-staging-values", "valuesKey": "values.yaml"}
    ]:
        problems.append(f"vault: values {spec.get('values')} {spec.get('valuesFrom')}")
    source = of_kind(docs, "ConfigMap").get("vault-staging-values") or {"data": {}}
    if yaml.safe_load(source["data"].get("values.yaml", "")) != yaml.safe_load(VAULT_VALUES.read_text()):
        problems.append("vault: values are not deploy/cell/values/vault-staging.yaml")
    annotations = release["metadata"].get("annotations") or {}
    if {k: annotations.get(k) for k in PRUNE_DISABLED} != PRUNE_DISABLED:
        problems.append("vault: prunable")
    return problems


def never_pruned(docs: list[dict]) -> list[str]:
    """Removing a namespace or the journal from git never deletes the journal's data: every
    namespace and the journal's release are unprunable, and the two the journal needs exist."""
    problems = []
    namespaces = set(of_kind(docs, "Namespace")) | {"cnpg-system", "ws-ws1"}
    keep = [("Namespace", name) for name in sorted(namespaces)] + [("HelmRelease", "journal")]
    for kind, name in keep:
        found = of_kind(docs, kind).get(name)
        annotations = (found or {}).get("metadata", {}).get("annotations") or {}
        if found is None or {k: annotations.get(k) for k in PRUNE_DISABLED} != PRUNE_DISABLED:
            problems.append(f"{kind} {name}: prunable")
    return problems


def strings(value, path=""):
    """Every string value and every key of a values tree, with its path."""
    if isinstance(value, dict):
        for key, item in value.items():
            yield f"{path}.{key}", str(key)
            yield from strings(item, f"{path}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            yield from strings(item, f"{path}[{index}]")
    elif isinstance(value, str):
        yield path, value


# A process image a hosted cluster names: a release binary's index under this repository's GHCR
# path, by digest alone (DEC-2016, DEC-2026); `promote` writes only these.
OWN_IMAGE = re.compile(r"ghcr\.io/kunwarshivam/mandate/mandate[a-z-]*@sha256:[0-9a-f]{64}")


# A ULID, as the API server's org and workspace ids are (MANDATE_API_ORG, MANDATE_API_WORKSPACE).
ULID = re.compile(r"[0-9A-HJKMNP-TV-Z]{26}")
API_URLS = ("issuer", "jwksUrl", "appOrigin")


def api_settings(values: dict[str, dict]) -> list[str]:
    """No placeholder anywhere in a hosted workspace's values (`placeholder`, as in
    `placeholder.invalid`, or the all-zero ULID); and once the API server's settings
    are written, each is set: its org and workspace as ULIDs, its issuer, key set and app origin
    over https, the key set under the issuer, and an audience."""
    workspace = values.get("workspace", {})
    problems = [
        f"workspace: {path} is a placeholder"
        for path, text in strings(workspace)
        if "placeholder" in text.lower() or text == "0" * 26
    ]
    api = {key: str(value) for key, value in (workspace.get("api") or {}).items() if value}
    named = (workspace.get("images") or {}).get("api")
    if not named and not set(api) - {"subjectLinks"}:
        return problems
    for key in ("org", "workspace"):
        if not ULID.fullmatch(api.get(key, "")):
            problems.append(f"workspace: api.{key} is not a ULID")
    for key in API_URLS:
        if not api.get(key, "").startswith("https://"):
            problems.append(f"workspace: api.{key} is not an https URL")
    if not api.get("audience"):
        problems.append("workspace: api.audience is not set")
    if not api.get("jwksUrl", "").startswith(api.get("issuer", "") + "/"):
        problems.append("workspace: api.jwksUrl is not the issuer's own key set")
    return problems


def values_policy(values: dict[str, dict], environment: str) -> list[str]:
    """No stub tier, no `live`, the paper environment, every image by digest, two journal instances
    on separate nodes."""
    problems = []
    for name, tree in values.items():
        for path, text in strings(tree):
            if text.strip().lower() == "live":
                problems.append(f"{name}: {path} is live")
    workspace = values.get("workspace", {})
    if workspace.get("environment") != "paper":
        problems.append(f"workspace: environment {workspace.get('environment')}")
    tier = workspace.get("deploymentTier")
    if tier in STUB_TIERS:
        problems.append(f"workspace: deploymentTier {tier}, a stub tier (DEC-2005)")
    if tier != environment:
        problems.append(f"workspace: deploymentTier {tier} in {environment}")
    if workspace.get("localImages"):
        problems.append("workspace: local images allowed")
    if (workspace.get("api") or {}).get("vaultBinding") and environment != "staging":
        problems.append(f"workspace: the API's vault binding is on in {environment}, where only staging runs a vault (DEC-2338)")
    for image, reference in (workspace.get("images") or {}).items():
        if image == "stub-issuer" and reference:
            problems.append("workspace: the stub issuer is named")
        elif reference and not PINNED.search(reference):
            problems.append(f"workspace: image {image} not pinned by digest: {reference}")
        elif reference and image != "postgres" and not OWN_IMAGE.fullmatch(reference):
            problems.append(f"workspace: image {image} is not this repository's release: {reference}")
    for name in ("cnpg", "barman-cloud"):
        tree = values.get(name, {})
        for key in ("image", "sidecarImage"):
            tag = (tree.get(key) or {}).get("tag")
            if (key == "image" or tag is not None) and not (tag and PINNED.search(tag)):
                problems.append(f"{name}: {key} not pinned by digest: {tag}")
    admission = values.get("policy-controller", {})
    webhook = admission.get("webhook") or {}
    if webhook.get("failurePolicy") != "Fail":
        problems.append("policy-controller: an outage of the pod webhooks admits any image")
    if (webhook.get("configData") or {}).get("no-match-policy") != "allow":
        problems.append("policy-controller: an image no policy matches is not simply allowed")
    for key in ("webhook", "leasescleanup"):
        version = ((admission.get(key) or {}).get("image") or {}).get("version", "")
        if not DIGEST.match(str(version)):
            problems.append(f"policy-controller: {key} image not pinned by digest: {version}")
    journal = values.get("journal", {})
    if journal.get("instances") != 2 or journal.get("failureDomain") != "separate":
        problems.append("journal: not two instances on separate nodes")
    archive = journal.get("archive") or {}
    if not R2_ENDPOINT.match(str(archive.get("endpointURL", ""))):
        problems.append(f"journal: archives to {archive.get('endpointURL')}, not R2's S3 endpoint")
    if archive.get("destinationPath") != f"s3://mandate-journal-archive-west-{environment}/journal-db":
        problems.append(f"journal: archives {environment} under {archive.get('destinationPath')}")
    if archive.get("egress") != R2_EGRESS:
        problems.append(f"journal: archive egress {archive.get('egress')}, not R2's two addresses on 443")
    return problems


def containers(doc: dict):
    """Every container of a rendered object, its pod templates' included."""
    found = []

    def walk(node):
        if isinstance(node, dict):
            for key in ("containers", "initContainers"):
                if isinstance(node.get(key), list):
                    found.extend(node[key])
            for item in node.values():
                walk(item)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(doc)
    return found


def rendered_policy(rendered: list[dict], environment: str) -> list[str]:
    """What the charts render: every image by digest, and no process told a stub tier, another
    environment's tier, or `live`."""
    problems = []
    for doc in rendered:
        where = f"{doc.get('kind')} {doc.get('metadata', {}).get('name')}"
        for container in containers(doc):
            image = container.get("image", "")
            if not PINNED.search(image):
                problems.append(f"{where}: image {image} not pinned by digest")
            for env in container.get("env") or []:
                value = str(env.get("value", ""))
                if env.get("name") == "MANDATE_DEPLOYMENT_TIER" and value != environment:
                    problems.append(f"{where}: tier {value}")
                if value.strip().lower() == "live":
                    problems.append(f"{where}: {env.get('name')} is live")
        image = doc.get("spec", {}).get("imageName")
        if image is not None and not PINNED.search(image):
            problems.append(f"{where}: imageName {image} not pinned by digest")
    return problems


@pytest.fixture(scope="module", params=OVERLAYS, ids=lambda o: str(o[1].relative_to(ROOT)))
def overlay(request, tmp_path_factory):
    environment, directory = request.param
    tmp = tmp_path_factory.mktemp(environment)
    docs = build(environment, directory, tmp)
    releases = of_kind(docs, "HelmRelease")
    values = {name: release_values(docs, release) for name, release in releases.items()}
    rendered = []
    for name, release in releases.items():
        if release["spec"].get("chartRef", {}).get("name") in OWN_CHARTS:
            rendered.extend(render(release, values[name], tmp))
    return environment, docs, values, rendered, directory


def admission_policy(policies: list[dict], docs: list[dict]) -> list[str]:
    """The one admission policy demands release.yml's signature on main of every image of this
    repository's, and the workspace's namespace is the one checked."""
    problems = []
    found = [d for d in policies if d.get("kind") == "ClusterImagePolicy"]
    if len(found) != 1:
        return [f"{len(found)} ClusterImagePolicies"]
    spec = found[0]["spec"]
    if spec.get("images") != ADMITTED_IMAGES:
        problems.append(f"the policy covers {spec.get('images')}")
    if spec.get("authorities") != [ADMISSION_AUTHORITY]:
        problems.append(f"the policy trusts {spec.get('authorities')}")
    labels = (of_kind(docs, "Namespace").get("ws-ws1") or {}).get("metadata", {}).get("labels") or {}
    if {k: labels.get(k) for k in ADMISSION_LABEL} != ADMISSION_LABEL:
        problems.append("ws-ws1 is not checked at admission")
    return problems


# The one exemption from the pod webhooks: the kill switch's pods (DEC-2016 item 2).
CONTROL_EXEMPT = {
    "matchExpressions": [{"key": "mandate.dev/process", "operator": "NotIn", "values": ["control"]}]
}
CONTROL_LABEL = "has({0}) && 'mandate.dev/process' in {0} && {0}['mandate.dev/process'] == 'control'"
JOB_CONTROLLER = "['system:serviceaccount:kube-system:job-controller', 'system:kube-controller-manager']"


def kill_switch_exemption(docs: list[dict]) -> list[str]:
    """Both pod webhooks skip exactly the control label, and only the Job controller, for a Job no
    service account made, can put that label on a pod in the workspace's namespace."""
    problems = []
    release = of_kind(docs, "HelmRelease").get("policy-controller") or {"spec": {}}
    patched = {}
    for renderer in release["spec"].get("postRenderers", []):
        for patch in (renderer.get("kustomize") or {}).get("patches", []):
            target = patch.get("target", {})
            for op in yaml.safe_load(patch.get("patch", "[]")) or []:
                patched.setdefault((target.get("kind"), target.get("name")), []).append(op)
    for kind in ("MutatingWebhookConfiguration", "ValidatingWebhookConfiguration"):
        ops = patched.pop((kind, "policy.sigstore.dev"), [])
        expected = [{"op": "add", "path": "/webhooks/0/objectSelector", "value": CONTROL_EXEMPT}]
        if ops != expected:
            problems.append(f"{kind} policy.sigstore.dev: exempts {ops}, not only the control label")
    if patched:
        problems.append(f"policy-controller: other post-render patches {sorted(patched)}")
    policies = of_kind(docs, "ValidatingAdmissionPolicy")
    bindings = of_kind(docs, "ValidatingAdmissionPolicyBinding")
    squash = " ".join
    pods = (policies.get("mandate-control-pods") or {}).get("spec", {})
    jobs = (policies.get("mandate-control-jobs") or {}).get("spec", {})
    expected = {
        "mandate-control-pods": (
            pods,
            ["pods", "pods/ephemeralcontainers"],
            "",
            CONTROL_LABEL.format("object.metadata.labels"),
            f"request.userInfo.username in {JOB_CONTROLLER} && has(object.metadata.ownerReferences) "
            "&& object.metadata.ownerReferences.exists(r, r.apiVersion == 'batch/v1' && r.kind == 'Job' "
            "&& has(r.controller) && r.controller)",
            ["CREATE", "UPDATE"],
            "request.operation == 'CREATE' || object.spec != oldObject.spec || !("
            + CONTROL_LABEL.format("oldObject.metadata.labels")
            + ")",
        ),
        "mandate-control-jobs": (
            jobs,
            ["jobs"],
            "batch",
            CONTROL_LABEL.format("object.spec.template.metadata.labels"),
            "!request.userInfo.username.startsWith('system:serviceaccount:') "
            "&& !(has(object.metadata.ownerReferences) "
            "&& object.metadata.ownerReferences.exists(r, r.kind == 'CronJob'))",
            ["CREATE", "UPDATE"],
            "request.operation == 'CREATE' || object.spec != oldObject.spec",
        ),
    }
    for name, (spec, resources, group, label, allowed, operations, changed) in expected.items():
        constraints = spec.get("matchConstraints", {})
        rules = constraints.get("resourceRules", [])
        if spec.get("failurePolicy") != "Fail":
            problems.append(f"{name}: does not fail closed")
        if constraints.get("namespaceSelector") != {"matchLabels": ADMISSION_LABEL}:
            problems.append(f"{name}: not every namespace the signature webhooks check")
        if [(r.get("apiGroups"), r.get("resources"), sorted(r.get("operations", []))) for r in rules] != [
            ([group], resources, operations)
        ]:
            problems.append(f"{name}: matches {rules}")
        conditions = {
            c.get("name"): squash(c.get("expression", "").split()) for c in spec.get("matchConditions", [])
        }
        if (
            set(conditions) != {"labelled-control", "changed-control"}
            or conditions["labelled-control"] != label
        ):
            problems.append(f"{name}: matches {conditions.get('labelled-control')!r}, not the control label")
        elif conditions["changed-control"] != changed:
            problems.append(
                f"{name}: matches {conditions['changed-control']!r}, not every change to a control spec"
            )
        validations = [squash(v.get("expression", "").split()) for v in spec.get("validations", [])]
        if validations != [allowed]:
            problems.append(f"{name}: allows {validations}")
        binding = (bindings.get(name) or {}).get("spec", {})
        if binding != {"policyName": name, "validationActions": ["Deny"]}:
            problems.append(f"{name}: bound {binding}, not denying")
    return problems


def test_kill_switch_exemption(overlay):
    assert kill_switch_exemption(overlay[1]) == []


# What reaches into a running pod. Exec, attach, port-forward and proxy are CONNECT requests whose
# admission object is their options, not the pod, so no policy can tell a kill switch's pod from
# another: RBAC holds them, and nothing the overlays or this repository's charts apply grants them
# (DEC-2046). The third-party charts (policy-controller, CloudNativePG) are not rendered here, and a
# role is read by its own rules, so an aggregated ClusterRole's borrowed rules are not seen.
POD_ACCESS = {
    "pods/exec",
    "pods/attach",
    "pods/portforward",
    "pods/proxy",
    "pods/ephemeralcontainers",
    "pods/*",
    "*",
}
BROAD_ROLES = {"cluster-admin", "admin", "edit"}


def pod_access(objects: list[dict]) -> list[str]:
    """No Role or ClusterRole the overlays or this repository's charts apply reaches into a pod, and no
    binding hands out a built-in role that does."""
    problems = []
    for doc in objects:
        where = f"{doc.get('kind')} {doc.get('metadata', {}).get('name')}"
        if doc.get("kind") in ("Role", "ClusterRole"):
            for rule in doc.get("rules") or []:
                groups = set(rule.get("apiGroups") or [])
                reached = POD_ACCESS & set(rule.get("resources") or [])
                if groups & {"", "*"} and reached:
                    problems.append(f"{where}: grants {sorted(reached)}")
        if doc.get("kind") in ("RoleBinding", "ClusterRoleBinding"):
            role = (doc.get("roleRef") or {}).get("name")
            if role in BROAD_ROLES:
                problems.append(f"{where}: binds {role}")
    return problems


def test_nothing_reaches_into_a_pod(overlay):
    _, docs, _, rendered, _ = overlay
    assert pod_access(docs + rendered) == []


@pytest.mark.parametrize(
    "added",
    [
        {
            "kind": "Role",
            "metadata": {"name": "debugger"},
            "rules": [{"apiGroups": [""], "resources": ["pods", "pods/exec"], "verbs": ["create"]}],
        },
        {
            "kind": "ClusterRole",
            "metadata": {"name": "everything"},
            "rules": [{"apiGroups": ["*"], "resources": ["*"], "verbs": ["*"]}],
        },
        {
            "kind": "Role",
            "metadata": {"name": "debug-container"},
            "rules": [{"apiGroups": [""], "resources": ["pods/ephemeralcontainers"], "verbs": ["patch"]}],
        },
        {
            "kind": "RoleBinding",
            "metadata": {"name": "editor"},
            "roleRef": {"kind": "ClusterRole", "name": "edit"},
        },
        {
            "kind": "Role",
            "metadata": {"name": "tunnel"},
            "rules": [
                {"apiGroups": [""], "resources": ["pods/portforward", "pods/proxy"], "verbs": ["create"]}
            ],
        },
    ],
)
def test_access_into_a_pod_is_caught(overlay, added):
    _, docs, _, rendered, _ = overlay
    assert pod_access([*docs, *rendered, added])


def uncovered_workspaces(docs: list[dict]) -> list[str]:
    """Every workspace's namespace (`ws-*`) carries the label that puts it under the signature
    webhooks and the kill-switch policies, so a new workspace cannot land unchecked."""
    return sorted(
        name
        for name, namespace in of_kind(docs, "Namespace").items()
        if name.startswith("ws-")
        and {k: (namespace["metadata"].get("labels") or {}).get(k) for k in ADMISSION_LABEL}
        != ADMISSION_LABEL
    )


def test_every_workspace_namespace_is_covered(overlay):
    assert uncovered_workspaces(overlay[1]) == []


def test_an_uncovered_workspace_namespace_is_caught(overlay):
    added = {"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": "ws-ws2"}}
    assert uncovered_workspaces([*overlay[1], added]) == ["ws-ws2"]


def policies() -> list[dict]:
    kustomization = yaml.safe_load((POLICIES / "kustomization.yaml").read_text())
    docs = []
    for resource in kustomization["resources"]:
        docs.extend(d for d in yaml.safe_load_all((POLICIES / resource).read_text()) if d)
    return docs


def test_admission_policy(overlay):
    assert admission_policy(policies(), overlay[1]) == []


ADMISSION_BUGS = [
    lambda p, d: p[0]["spec"].update(images=[{"glob": "ghcr.io/kunwarshivam/mandate/mandate"}]),
    lambda p, d: p[0]["spec"]["authorities"][0]["keyless"]["identities"][0].update(subjectRegExp=".*"),
    lambda p, d: p[0]["spec"]["authorities"][0].pop("ctlog"),
    lambda p, d: next(n for n in d if n["metadata"]["name"] == "ws-ws1")["metadata"].pop("labels"),
    lambda p, d: p.append(copy.deepcopy(p[0])),
]


@pytest.mark.parametrize("change", ADMISSION_BUGS)
def test_each_admission_seeded_bug_is_caught(change, tmp_path):
    docs = build("staging", FIXTURE, tmp_path)
    bad_policies, bad_docs = copy.deepcopy(policies()), copy.deepcopy(docs)
    change(bad_policies, bad_docs)
    assert admission_policy(bad_policies, bad_docs)


def test_sources_pinned(overlay):
    assert sources_pinned(overlay[1], overlay[0]) == []


def test_vault_release(overlay):
    assert vault_release(overlay[1], overlay[0]) == []


def vault_reviewer(rendered: list[dict], environment: str = "staging") -> list[str]:
    """Only where the vault runs, staging, is there one binding that lets it review tokens: the
    vault's own ServiceAccount in ws-ws1, bound to `system:auth-delegator` (DEC-2062), rendered
    together with the token it reviews with and its egress to the API server. Production renders
    no vault object at all."""
    problems = []
    vault = [d for d in rendered if (d.get("metadata", {}).get("labels") or {}).get("mandate.dev/process") == "vault"]
    if environment != "staging":
        problems += [f"{environment}: renders the vault's {d['kind']} {d['metadata']['name']}" for d in vault]
    else:
        sets = [d for d in vault if d.get("kind") == "StatefulSet"]
        volumes = [v["name"] for s in sets for v in s["spec"]["template"]["spec"].get("volumes", [])]
        egress = [e for d in vault if d.get("kind") == "NetworkPolicy" for e in d["spec"].get("egress") or []]
        if "kube-api-token" not in volumes or not egress:
            problems.append(f"staging: the vault's token volume {volumes} and egress {egress} are not both there")
    bindings = [d for d in rendered if d.get("kind") == "ClusterRoleBinding"]
    want = (
        [
            {
                "name": "vault-token-review-ws-ws1",
                "roleRef": "system:auth-delegator",
                "subjects": [{"kind": "ServiceAccount", "name": "vault", "namespace": "ws-ws1"}],
            }
        ]
        if environment == "staging"
        else []
    )
    got = [
        {"name": b["metadata"]["name"], "roleRef": b["roleRef"]["name"], "subjects": b.get("subjects")}
        for b in bindings
    ]
    if got != want:
        problems.append(f"{environment}: the review bindings are {got}, not {want}")
    return problems


def test_only_staging_lets_the_vault_review_tokens(overlay):
    environment, _, _, rendered, _ = overlay
    assert vault_reviewer(rendered, environment) == []


def test_a_review_binding_out_of_place_is_caught(overlay):
    environment, _, _, rendered, _ = overlay
    staged = [d for d in rendered if d.get("kind") == "ClusterRoleBinding"]
    if environment == "staging":
        assert vault_reviewer(rendered, "production"), "production must refuse staging's binding"
        assert vault_reviewer([d for d in rendered if d not in staged], "staging")
        widened = copy.deepcopy(rendered)
        for d in widened:
            if d.get("kind") == "ClusterRoleBinding":
                d["subjects"].append({"kind": "Group", "name": "system:authenticated"})
        assert vault_reviewer(widened, "staging")
        closed = copy.deepcopy(rendered)
        for d in closed:
            if d.get("kind") == "NetworkPolicy" and d["metadata"]["name"] == "vault":
                d["spec"]["egress"] = []
        assert vault_reviewer(closed, "staging"), "a binding without the egress is caught"
    else:
        stray = {
            "kind": "ClusterRoleBinding",
            "metadata": {"name": "vault-token-review-ws-ws1"},
            "roleRef": {"name": "system:auth-delegator"},
            "subjects": [{"kind": "ServiceAccount", "name": "vault", "namespace": "ws-ws1"}],
        }
        assert vault_reviewer([*rendered, stray], environment)


def test_production_running_the_vault_is_caught(tmp_path):
    docs = build("staging", FIXTURE, tmp_path)
    assert sources_pinned(docs, "production") == ["mandate-vault-server: runs in staging alone (DEC-2051)"]
    assert vault_release(docs, "production") == ["vault: runs in staging alone (DEC-2051)"]


def test_releases_resolve(overlay):
    assert releases_resolve(overlay[1]) == []


def test_sync_read_only(overlay):
    assert sync_read_only(overlay[1], overlay[4]) == []


def test_endpoint_from_bootstrap(overlay):
    assert endpoint_from_bootstrap(overlay[1]) == []


def test_never_pruned(overlay):
    assert never_pruned(overlay[1]) == []


def test_api_settings(overlay):
    assert api_settings(overlay[2]) == []


@pytest.mark.parametrize(
    "api",
    [
        {"issuer": "https://placeholder.invalid"},
        {"org": "0" * 26},
        {"org": "01KDEM0000000000000000000R", "workspace": "ws1"},
        {"issuer": "http://issuer.example"},
        {"appOrigin": "https://app.example", "audience": ""},
        {"jwksUrl": "https://other.example/jwks.json"},
    ],
)
def test_each_api_settings_bug_is_caught(overlay, api):
    good = {
        "org": "01KDEM0000000000000000000R",
        "workspace": "01KDEM0000000000000000000W",
        "issuer": "https://issuer.example",
        "audience": "authenticated",
        "jwksUrl": "https://issuer.example/jwks.json",
        "appOrigin": "https://app.example",
    }
    values = copy.deepcopy(overlay[2])
    values["workspace"] = merge(values["workspace"], {"api": good})
    assert api_settings(values) == []
    values["workspace"] = merge(values["workspace"], {"api": api})
    assert api_settings(values), f"api_settings missed {api}"


def test_values_policy(overlay):
    assert values_policy(overlay[2], overlay[0]) == []


def test_rendered_policy(overlay):
    environment, _, _, rendered, _ = overlay
    assert {d["kind"] for d in rendered} >= {"Cluster", "NetworkPolicy", "Deployment"}
    assert rendered_policy(rendered, environment) == []


def test_rendered_objects_match_the_kubernetes_schemas(overlay, tmp_path):
    environment, docs, _, rendered, _ = overlay
    plain = [
        d
        for d in docs
        if d["kind"]
        in ("Namespace", "ConfigMap", "ValidatingAdmissionPolicy", "ValidatingAdmissionPolicyBinding")
    ]
    manifest = tmp_path / f"{environment}.yaml"
    manifest.write_text(yaml.safe_dump_all(plain + rendered))
    done = subprocess.run(
        [
            "kubeconform",
            "-strict",
            "-summary",
            "-kubernetes-version",
            "1.35.0",
            "-skip",
            NO_SCHEMA,
            str(manifest),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode == 0, done.stdout + done.stderr


# What deploy/gitops/ may hold: the base, one overlay per hosted environment, the admission
# policies, and the runbook. A
# manifest anywhere else would be reconciled by nothing, or checked by nothing.
# The external hosts each process may reach in a hosted cluster (DEC-2058), written out here from the
# decision, not read from the overlay: infrastructure §3.1's external slots, as names. Alpaca's live
# host is the token-exchange process's alone, for its one OAuth call (DEC-821 items 2 and 4, DEC-2059);
# every other process trades on paper only (AGENTS.md rule 8).
FQDN_EGRESS = {
    "api": {"wyyxngmblqqokcvraflx.supabase.co"},
    "control": {"wyyxngmblqqokcvraflx.supabase.co"},
    "executor": {"paper-api.alpaca.markets"},
    "data": {"data.alpaca.markets", "stream.data.alpaca.markets"},
    "model-gateway": {"openrouter.ai"},
    "token-exchange": {"api.alpaca.markets"},
}
LIVE_HOST = re.compile(r"(?<![\w.-])api\.alpaca\.markets\b", re.IGNORECASE)
# Every kind that can grant egress, and the one a hosted overlay may hold: the chart renders its own
# NetworkPolicies, which the overlay's build never holds, so any other here is a back door.
EGRESS_KINDS = {"NetworkPolicy", "CiliumNetworkPolicy", "CiliumClusterwideNetworkPolicy"}
KUBE_DNS = {"io.kubernetes.pod.namespace": "kube-system", "k8s-app": "kube-dns"}


def fqdn_egress(docs: list[dict], hosts: dict[str, set[str]]) -> list[str]:
    """Every CiliumNetworkPolicy in a hosted overlay lets exactly one process of the workspace reach
    exactly its own hosts, by exact name on TCP 443, after asking the cluster DNS through Cilium's
    proxy; nothing else, no ingress. Every process `hosts` names has one. Alpaca's live host appears
    once at most, and only as an exact name in the one policy that selects the workspace's
    token-exchange pods (DEC-2059)."""
    problems = [
        f"{d['kind']} {d['metadata']['name']} may grant egress"
        for d in docs
        if d["kind"] in EGRESS_KINDS
        and not (
            d["kind"] == "CiliumNetworkPolicy"
            and d["apiVersion"] == "cilium.io/v2"
            and d["metadata"]["name"]
            in {f"egress-fqdn-{process}" for process in hosts}
        )
    ]
    live = sum(len(LIVE_HOST.findall(str(d))) for d in docs)
    granted = [
        d
        for d in docs
        if d["kind"] == "CiliumNetworkPolicy"
        and d["spec"].get("endpointSelector", {}).get("matchLabels")
        == {"mandate.dev/workspace": "ws1", "mandate.dev/process": "token-exchange"}
        and any(
            f == {"matchName": "api.alpaca.markets"}
            for r in d["spec"].get("egress", [])
            for f in r.get("toFQDNs", [])
        )
    ]
    if live != len(granted) or len(granted) > 1:
        problems.append(f"api.alpaca.markets appears {live} times, granted to the token exchange {len(granted)} times")
    seen = set()
    for policy in [d for d in docs if d["kind"] == "CiliumNetworkPolicy"]:
        name = policy["metadata"]["name"]
        spec = policy["spec"]
        selector = spec.get("endpointSelector", {}).get("matchLabels", {})
        process = selector.get("mandate.dev/process")
        if policy["apiVersion"] != "cilium.io/v2" or policy["metadata"].get("namespace") != "ws-ws1":
            problems.append(f"{name} is not a cilium.io/v2 policy in ws-ws1")
        if selector != {"mandate.dev/workspace": "ws1", "mandate.dev/process": process} or process not in hosts:
            problems.append(f"{name} selects {selector}")
            continue
        seen.add(process)
        if set(spec) != {"endpointSelector", "egress"}:
            problems.append(f"{name} sets {sorted(set(spec) - {'endpointSelector', 'egress'})}")
        dns_rule = {
            "toEndpoints": [{"matchLabels": KUBE_DNS}],
            "toPorts": [{"ports": [{"port": "53", "protocol": "ANY"}], "rules": {"dns": [{"matchPattern": "*"}]}}],
        }
        rules = spec.get("egress", [])
        named = [r for r in rules if r != dns_rule]
        if len(rules) - len(named) != 1 or len(named) != 1:
            problems.append(f"{name} is not one DNS rule and one host rule")
            continue
        rule = named[0]
        names = {str(f.get("matchName")).lower() for f in rule.get("toFQDNs", [])}
        if set(rule) != {"toFQDNs", "toPorts"} or any(set(f) != {"matchName"} for f in rule.get("toFQDNs", [])):
            problems.append(f"{name} reaches more than exact names: {rule}")
        if rule.get("toPorts") != [{"ports": [{"port": "443", "protocol": "TCP"}]}]:
            problems.append(f"{name} reaches ports {rule.get('toPorts')}")
        if names != hosts[process]:
            problems.append(f"{name} reaches {sorted(names)}, want {sorted(hosts[process])}")
    if hosts and seen != set(hosts):
        problems.append(f"FQDN policies for {sorted(seen)}, want {sorted(hosts)}")
    return problems


def test_fqdn_egress(overlay):
    environment, docs, *_ = overlay
    assert fqdn_egress(docs, FQDN_EGRESS if environment == "staging" and overlay[4] != FIXTURE else {}) == []


def test_each_fqdn_egress_bug_is_caught(tmp_path):
    docs = build("staging", GITOPS / "clusters" / "staging", tmp_path)
    assert fqdn_egress(docs, FQDN_EGRESS) == []

    def policy(process: str) -> dict:
        return next(
            d
            for d in docs
            if d["kind"] == "CiliumNetworkPolicy"
            and d["spec"]["endpointSelector"]["matchLabels"]["mandate.dev/process"] == process
        )

    def clusterwide(rule: dict) -> dict:
        return {
            "apiVersion": "cilium.io/v2",
            "kind": "CiliumClusterwideNetworkPolicy",
            "metadata": {"name": "wide"},
            "spec": {"endpointSelector": {}, "egress": [rule]},
        }

    def host_rule(process: str) -> dict:
        return next(r for r in policy(process)["spec"]["egress"] if "toFQDNs" in r)

    seeds = [
        lambda: host_rule("executor")["toFQDNs"].append({"matchName": "api.alpaca.markets"}),
        lambda: host_rule("data")["toFQDNs"].append({"matchPattern": "*.alpaca.markets"}),
        lambda: host_rule("model-gateway")["toFQDNs"].append({"matchName": "paper-api.alpaca.markets"}),
        lambda: host_rule("executor")["toPorts"][0]["ports"].append({"port": "80", "protocol": "TCP"}),
        lambda: policy("api")["spec"]["egress"].append({"toCIDR": ["0.0.0.0/0"]}),
        lambda: policy("api")["spec"]["endpointSelector"]["matchLabels"].update({"mandate.dev/process": "runtime"}),
        lambda: policy("control")["spec"]["endpointSelector"]["matchLabels"].pop("mandate.dev/workspace"),
        lambda: policy("data")["spec"].update(ingress=[{}]),
        lambda: docs.remove(policy("model-gateway")),
        lambda: host_rule("data")["toFQDNs"].append({"matchName": "api.alpaca.markets"}),
        lambda: host_rule("token-exchange")["toFQDNs"].append({"matchName": "paper-api.alpaca.markets"}),
        lambda: host_rule("token-exchange")["toPorts"][0]["ports"].append({"port": "80", "protocol": "TCP"}),
        lambda: host_rule("token-exchange")["toFQDNs"].append({"matchPattern": "*.alpaca.markets"}),
        lambda: policy("token-exchange")["spec"]["egress"].append({"toCIDR": ["0.0.0.0/0"]}),
        lambda: policy("token-exchange")["spec"]["endpointSelector"]["matchLabels"].pop("mandate.dev/workspace"),
        lambda: docs.append(
            {
                **copy.deepcopy(policy("token-exchange")),
                "metadata": {"name": "egress-fqdn-token-exchange-ws2", "namespace": "ws-ws1"},
                "spec": {
                    **copy.deepcopy(policy("token-exchange")["spec"]),
                    "endpointSelector": {
                        "matchLabels": {"mandate.dev/workspace": "ws2", "mandate.dev/process": "token-exchange"}
                    },
                },
            }
        ),
        lambda: docs.append({**copy.deepcopy(policy("token-exchange")), "metadata": {"name": "twice", "namespace": "ws-ws1"}}),
        lambda: docs.append({"kind": "ConfigMap", "apiVersion": "v1", "metadata": {"name": "x"}, "data": {"h": "api.alpaca.markets"}}),
        lambda: docs.append(clusterwide({"toEntities": ["world"]})),
        lambda: docs.append(clusterwide({"toCIDR": ["0.0.0.0/0"]})),
        lambda: docs.append(clusterwide({"toFQDNs": [{"matchPattern": "*"}]})),
        lambda: docs.append(
            {
                "apiVersion": "networking.k8s.io/v1",
                "kind": "NetworkPolicy",
                "metadata": {"name": "wide", "namespace": "ws-ws1"},
                "spec": {"podSelector": {}, "policyTypes": ["Egress"], "egress": [{"to": [{"ipBlock": {"cidr": "0.0.0.0/0"}}]}]},
            }
        ),
        lambda: docs.append({"kind": "ConfigMap", "apiVersion": "v1", "metadata": {"name": "y"}, "data": {"h": "API.Alpaca.Markets"}}),
    ]
    pristine = copy.deepcopy(docs)
    for seed in seeds:
        docs[:] = copy.deepcopy(pristine)
        seed()
        assert fqdn_egress(docs, FQDN_EGRESS), seed


GITOPS_ENTRIES = {"base", "clusters", "policies", "BOOTSTRAP.md"}


def test_every_overlay_is_a_hosted_environment():
    clusters = GITOPS / "clusters"
    found = sorted(p.name for p in clusters.iterdir()) if clusters.is_dir() else []
    assert set(found) <= set(ENVIRONMENTS), found
    entries = {p.name for p in GITOPS.iterdir()} if GITOPS.is_dir() else set()
    assert entries <= GITOPS_ENTRIES, sorted(entries - GITOPS_ENTRIES)


def seeded(docs: list[dict], kind: str, name: str, change) -> list[dict]:
    out = copy.deepcopy(docs)
    for doc in out:
        if doc["kind"] == kind and doc["metadata"]["name"] == name:
            change(doc)
    return out


DOC_BUGS = [
    (sources_pinned, "OCIRepository", "mandate-workspace", lambda d: d["spec"].update(ref={"tag": "latest"})),
    (sources_pinned, "OCIRepository", "mandate-journal-db", lambda d: d["spec"].pop("verify")),
    (sources_pinned, "OCIRepository", "mandate-vault-server", lambda d: d["spec"].pop("verify")),
    (vault_release, "HelmRelease", "vault", lambda d: d["spec"].pop("install")),
    (vault_release, "HelmRelease", "vault", lambda d: d["spec"]["upgrade"].update(disableWait=False)),
    (vault_release, "HelmRelease", "vault", lambda d: d["spec"].update(targetNamespace="vault")),
    (vault_release, "HelmRelease", "vault", lambda d: d["metadata"].pop("annotations")),
    (vault_release, "HelmRelease", "vault", lambda d: d["spec"].update(values={"seal": {"type": "shamir"}})),
    (
        vault_release,
        "ConfigMap",
        "vault-staging-values",
        lambda d: d["data"].update(
            {"values.yaml": d["data"]["values.yaml"].replace("staging-1", "staging-2")}
        ),
    ),
    (
        sources_pinned,
        "OCIRepository",
        "mandate-workspace",
        lambda d: d["spec"]["verify"]["matchOIDCIdentity"][0].update(subject=".*"),
    ),
    (
        sources_pinned,
        "OCIRepository",
        "cloudnative-pg",
        lambda d: d["spec"]["ref"].update(digest="sha256:" + "1" * 64),
    ),
    (sources_pinned, "OCIRepository", "plugin-barman-cloud", lambda d: d["spec"].pop("layerSelector")),
    (
        sources_pinned,
        "OCIRepository",
        "cloudnative-pg",
        lambda d: d["spec"].update(url="oci://ghcr.io/elsewhere/charts/cloudnative-pg"),
    ),
    (
        sources_pinned,
        "OCIRepository",
        "mandate-workspace",
        lambda d: d["spec"].update(url="oci://ghcr.io/elsewhere/charts/mandate-workspace"),
    ),
    (
        sources_pinned,
        "OCIRepository",
        "mandate-journal-db",
        lambda d: d["spec"]["verify"].update(secretRef={"name": "cosign-key"}),
    ),
    # A namespace the journal needs is missing; the every-namespace rule has the ConfigMap case below.
    (never_pruned, "Namespace", "cnpg-system", lambda d: d["metadata"].update(name="extra")),
    (
        never_pruned,
        "ConfigMap",
        "workspace-managed-values",
        lambda d: d.update(kind="Namespace", metadata={"name": "extra"}),
    ),
    (
        releases_resolve,
        "HelmRelease",
        "journal",
        lambda d: d["spec"].update(chart={"spec": {"chart": "./deploy/cell/charts/journal-db"}}),
    ),
    (releases_resolve, "HelmRelease", "workspace", lambda d: d["spec"]["chartRef"].update(name="elsewhere")),
    (sync_read_only, "GitRepository", "mandate", lambda d: d["spec"].update(ref={"branch": "go-gitops"})),
    (
        sync_read_only,
        "GitRepository",
        "mandate",
        lambda d: d["spec"].update(secretRef={"name": "deploy-key"}),
    ),
    (sync_read_only, "Kustomization", "cluster", lambda d: d["spec"].update(path="./deploy/gitops")),
    (kill_switch_exemption, "HelmRelease", "policy-controller", lambda d: d["spec"].pop("postRenderers")),
    (
        kill_switch_exemption,
        "HelmRelease",
        "policy-controller",
        lambda d: d["spec"]["postRenderers"][0]["kustomize"]["patches"][0].update(
            patch=d["spec"]["postRenderers"][0]["kustomize"]["patches"][0]["patch"].replace("NotIn", "In")
        ),
    ),
    (
        kill_switch_exemption,
        "HelmRelease",
        "policy-controller",
        lambda d: d["spec"]["postRenderers"][0]["kustomize"]["patches"][1].update(
            patch=d["spec"]["postRenderers"][0]["kustomize"]["patches"][1]["patch"].replace(
                "[control]", "[control, api]"
            )
        ),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["validations"][0].update(expression="true"),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["matchConditions"][0].update(expression="false"),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["matchConditions"].pop(),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["matchConstraints"]["resourceRules"][0].update(operations=["CREATE"]),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["matchConditions"].pop(),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["matchConditions"][1].update(expression="true"),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["matchConditions"][1].update(
            expression="request.operation == 'CREATE' || !("
            + CONTROL_LABEL.format("oldObject.spec.template.metadata.labels")
            + ")"
        ),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["matchConditions"][1].update(
            expression=d["spec"]["matchConditions"][1]["expression"].replace(
                "object.spec != oldObject.spec || ", ""
            )
        ),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["matchConstraints"]["resourceRules"][0].update(resources=["pods"]),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["validations"][0].update(
            expression="!request.userInfo.username.startsWith('system:serviceaccount:')"
        ),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-pods",
        lambda d: d["spec"]["validations"][0].update(
            expression=d["spec"]["validations"][0]["expression"].split(" && has(")[0]
        ),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"].update(failurePolicy="Ignore"),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicyBinding",
        "mandate-control-jobs",
        lambda d: d["spec"].update(validationActions=["Warn"]),
    ),
    (
        kill_switch_exemption,
        "ValidatingAdmissionPolicy",
        "mandate-control-jobs",
        lambda d: d["spec"]["matchConstraints"].update(namespaceSelector={}),
    ),
    (endpoint_from_bootstrap, "HelmRelease", "journal", lambda d: d["spec"].pop("valuesFrom")),
    (
        endpoint_from_bootstrap,
        "HelmRelease",
        "journal",
        lambda d: (
            d["spec"]["values"]
            .setdefault("archive", {})
            .update(endpointURL="https://x.r2.cloudflarestorage.com")
        ),
    ),
    (
        endpoint_from_bootstrap,
        "HelmRelease",
        "journal",
        lambda d: next(r for r in d["spec"]["valuesFrom"] if r["name"] == "journal-archive-endpoint").update(
            optional=True
        ),
    ),
    (
        endpoint_from_bootstrap,
        "Namespace",
        "ws-ws1",
        lambda d: d.update(kind="ConfigMap", metadata={"name": "journal-archive-endpoint"}),
    ),
    (never_pruned, "Namespace", "ws-ws1", lambda d: d["metadata"].pop("annotations")),
    (never_pruned, "HelmRelease", "journal", lambda d: d["metadata"].pop("annotations")),
]


@pytest.mark.parametrize("check,kind,name,change", DOC_BUGS)
def test_each_overlay_seeded_bug_is_caught(check, kind, name, change, tmp_path):
    docs = build("staging", FIXTURE, tmp_path)
    bad = seeded(docs, kind, name, change)
    found = check(bad, FIXTURE) if check is sync_read_only else check(bad)
    assert found, f"{check.__name__} missed a seeded bug in {kind} {name}"


VALUE_BUGS = [
    ("workspace", {"images": {"mandate": "docker.io/library/busybox@sha256:" + "a" * 64}}),
    ("workspace", {"images": {"api": "ghcr.io/kunwarshivam/mandate/mandate:signed-abc"}}),
    ("workspace", {"images": {"runtime": "ghcr.io/someone/mandate/mandate@sha256:" + "a" * 64}}),
    ("workspace", {"deploymentTier": "dev"}),
    ("workspace", {"deploymentTier": "ci"}),
    ("workspace", {"environment": "live"}),
    ("workspace", {"api": {"issuer": "live"}}),
    ("workspace", {"localImages": True}),
    ("policy-controller", {"webhook": {"failurePolicy": "Ignore"}}),
    ("policy-controller", {"webhook": {"configData": {"no-match-policy": "deny"}}}),
    ("policy-controller", {"webhook": {"image": {"version": "v0.13.1"}}}),
    ("policy-controller", {"leasescleanup": {"image": {"version": "latest-dev"}}}),
    ("workspace", {"deploymentTier": None}),
    ("workspace", {"deploymentTier": "production-eu"}),
    ("workspace", {"images": {"stub-issuer": "mandate.local/stub-issuer:1"}}),
    ("workspace", {"images": {"api": "ghcr.io/kunwarshivam/mandate/mandate:main"}}),
    ("cnpg", {"image": {"tag": "1.30.1"}}),
    ("barman-cloud", {"sidecarImage": {"tag": "v0.15.1"}}),
    ("journal", {"instances": 1}),
    ("journal", {"failureDomain": "shared"}),
    ("journal", {"archive": {"endpointURL": "https://nyc3.digitaloceanspaces.com"}}),
    ("journal", {"archive": {"egress": {"cidrs": ["172.64.0.0/13"], "port": 443}}}),
    ("journal", {"archive": {"egress": {"cidrs": ["172.64.190.1/32", "172.64.66.1/32"], "port": 80}}}),
    ("journal", {"archive": {"destinationPath": "s3://mandate-journal-archive-elsewhere/journal-db"}}),
]


@pytest.mark.parametrize("release,change", VALUE_BUGS)
def test_each_values_seeded_bug_is_caught(overlay, release, change):
    environment, _, values, _, _ = overlay
    bad = copy.deepcopy(values)
    bad[release] = merge(bad[release], change)
    assert values_policy(bad, environment), f"values_policy missed {release} {change}"


def test_the_api_is_bound_to_the_vault_in_staging_alone(overlay):
    """DEC-2338: staging's API carries the vault binding, its AppRole copied owner-only and its
    egress to the vault on 8200; production, which runs no vault, carries none of it."""
    environment, _, values, rendered, directory = overlay
    bound = bool((values["workspace"].get("api") or {}).get("vaultBinding"))
    if directory == GITOPS / "clusters" / "staging":
        assert bound, "staging's API is bound to its vault"
    assert api_vault(rendered, on=bound) == []
    if environment != "staging":
        bad = copy.deepcopy(values)
        bad["workspace"] = merge(bad["workspace"], {"api": {"vaultBinding": True}})
        assert values_policy(bad, environment), "a binding where no vault runs is caught"


def test_another_environments_tier_is_caught(overlay):
    environment, _, values, _, _ = overlay
    other = next(e for e in ENVIRONMENTS if e != environment)
    bad = copy.deepcopy(values)
    bad["workspace"] = merge(bad["workspace"], {"deploymentTier": other})
    assert values_policy(bad, environment)


def test_rendered_seeded_bugs_are_caught(overlay):
    environment, _, _, rendered, _ = overlay
    deployment = next(d for d in rendered if d["kind"] == "Deployment")
    tagged = copy.deepcopy(deployment)
    containers(tagged)[0]["image"] = "cloudflare/cloudflared:2026.9.3"
    assert rendered_policy([tagged], environment)
    tiered = copy.deepcopy(deployment)
    containers(tiered)[0].setdefault("env", []).append({"name": "MANDATE_DEPLOYMENT_TIER", "value": "ci"})
    assert rendered_policy([tiered], environment)
    live = copy.deepcopy(deployment)
    containers(live)[0].setdefault("env", []).append({"name": "MANDATE_ENVIRONMENT", "value": "live"})
    assert rendered_policy([live], environment)
    cluster = copy.deepcopy(next(d for d in rendered if d["kind"] == "Cluster"))
    cluster["spec"]["imageName"] = "ghcr.io/cloudnative-pg/postgresql:18"
    assert rendered_policy([cluster], environment)


# An R2 endpoint naming an account: 32 hex digits before R2's S3 host.
ACCOUNT_ENDPOINT = re.compile(r"[0-9a-f]{32}\.r2\.cloudflarestorage\.com")


def accounts_in_git(files: dict[str, str]) -> list[str]:
    """No tracked file names a Cloudflare account in an R2 endpoint, but for this file's stand-in
    (DEC-2017 item 6): the founder's account reaches a cluster only through the bootstrap."""
    return sorted(
        f"{path}: {found}"
        for path, text in files.items()
        for found in ACCOUNT_ENDPOINT.findall(text)
        if found != f"{STAND_IN_ACCOUNT}.r2.cloudflarestorage.com"
    )


def tracked_files() -> dict[str, str]:
    listed = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True, cwd=ROOT).stdout
    files = {}
    for name in listed.decode().split("\0"):
        path = ROOT / name
        if name and path.is_file():
            files[name] = path.read_bytes().decode("utf-8", errors="replace")
    return files


def test_no_account_in_git():
    assert accounts_in_git(tracked_files()) == []


def test_an_account_in_git_is_caught():
    account = "fedcba9876543210" * 2
    endpoint = f"https://{account}.r2.cloudflarestorage.com"
    assert accounts_in_git({"deploy/gitops/clusters/staging/journal.yaml": f"endpointURL: {endpoint}\n"})


def test_an_extra_source_is_caught(tmp_path):
    """A source beside the pinned ones, every pinned one still present, is refused on its own."""
    docs = build("staging", FIXTURE, tmp_path)
    assert sources_pinned(docs) == []
    extra = copy.deepcopy(of_kind(docs, "OCIRepository")["mandate-workspace"])
    extra["metadata"]["name"] = "mandate-extra"
    assert sources_pinned([*docs, extra]) == [
        "mandate-extra: an OCIRepository the hosted clusters do not run"
    ]


# Production pins, per key, only what staging held on main (DEC-2016 item 5, DEC-2048).
STAGING_FILES = ("deploy/gitops/clusters/staging/charts.yaml", "deploy/gitops/clusters/staging/images.yaml")
PRODUCTION_FILES = (
    "deploy/gitops/clusters/production/charts.yaml",
    "deploy/gitops/clusters/production/images.yaml",
)
ANY_DIGEST = re.compile(r"sha256:[0-9a-f]{64}")


def pins(charts: str, images: str) -> set[tuple[str, str]]:
    """What an environment's two files pin, per key: each OCIRepository's name with its digest, and
    each image key with its reference."""
    found = set()
    for doc in yaml.safe_load_all(charts):
        if doc and doc.get("kind") == "OCIRepository":
            found.add((doc["metadata"]["name"], str((doc["spec"].get("ref") or {}).get("digest"))))
    for key, reference in ((yaml.safe_load(images) or {}).get("images") or {}).items():
        found.add((f"images.{key}", str(reference)))
    return found


def unstaged_pins(production: set[tuple[str, str]], staged: set[tuple[str, str]]) -> list[tuple[str, str]]:
    """Each pin production holds that staging never held under the same key."""
    return sorted(production - staged)


def git(*args: str) -> str:
    done = subprocess.run(["git", *args], capture_output=True, text=True, check=False, cwd=ROOT)
    assert done.returncode == 0, done.stderr
    return done.stdout


def staging_history(base: str) -> set[tuple[str, str]]:
    """Every pin staging's files held at each commit on main up to `base`."""
    held = set()
    for commit in git("rev-list", base, "--", *STAGING_FILES).split():
        texts = []
        for path in STAGING_FILES:
            shown = subprocess.run(
                ["git", "show", f"{commit}:{path}"], capture_output=True, text=True, check=False, cwd=ROOT
            )
            texts.append(shown.stdout if shown.returncode == 0 else "")
        held |= pins(*texts)
    return held


def test_production_runs_only_what_staging_ran():
    if subprocess.run(
        ["git", "rev-parse", "--verify", "-q", "origin/main"], cwd=ROOT, check=False
    ).returncode:
        if REQUIRED:
            raise RuntimeError("MANDATE_GITOPS_REQUIRED is set but origin/main is not fetched")
        pytest.skip("origin/main is not fetched")
    base = git("merge-base", "HEAD", "origin/main").strip()
    production = pins(*((ROOT / path).read_text() for path in PRODUCTION_FILES))
    assert production, "production pins nothing"
    assert unstaged_pins(production, staging_history(base)) == [], (
        "production pins what staging never ran on main"
    )


def test_a_production_pin_staging_never_ran_is_caught():
    ran, other = f"sha256:{'1' * 64}", f"sha256:{'2' * 64}"
    staged = {("mandate-workspace", ran), ("mandate-journal-db", other), ("images.mandate", f"x@{ran}")}
    assert unstaged_pins({("mandate-workspace", ran), ("images.mandate", f"x@{ran}")}, staged) == []
    assert unstaged_pins({("mandate-workspace", other)}, staged) == [("mandate-workspace", other)]
    assert unstaged_pins({("images.mandate", f"y@{ran}")}, staged) == [("images.mandate", f"y@{ran}")]


def test_pins_are_read_per_key():
    charts = (ROOT / PRODUCTION_FILES[0]).read_text()
    found = pins(charts, "images:\n  mandate: ghcr.io/x@sha256:" + "a" * 64 + "\n")
    assert ("images.mandate", "ghcr.io/x@sha256:" + "a" * 64) in found
    assert {key for key, _ in found} >= {"mandate-workspace", "mandate-journal-db", "images.mandate"}
    assert all(ANY_DIGEST.fullmatch(digest) for key, digest in found if not key.startswith("images."))


TOFU_STAGING = ROOT / "deploy" / "tofu" / "environments" / "staging" / "main.tf"


def staging_redirect_problems(overlay: str, main_tf: str) -> list[str]:
    """DEC-2636: staging's Alpaca OAuth redirect URI is the callback on the API host staging's
    OpenTofu root names, so a host rename cannot silently break the callback."""
    named = re.search(r'^\s*api_hostname\s*=\s*"([^"]+)"', main_tf, re.M)
    if not named:
        return ["staging's main.tf names no api_hostname"]
    uri = (yaml.safe_load(overlay)["spec"]["values"].get("alpacaOAuth") or {}).get("redirectUri", "")
    expected = f"https://{named.group(1)}/v1/oauth/alpaca/callback"
    return [] if uri == expected else [f"staging's redirect URI {uri!r} is not {expected!r}"]


def test_staging_s_alpaca_redirect_uri_is_on_its_api_host():
    overlay = (GITOPS / "clusters" / "staging" / "workspace.yaml").read_text()
    main_tf = TOFU_STAGING.read_text()
    assert staging_redirect_problems(overlay, main_tf) == []
    assert staging_redirect_problems(overlay, main_tf.replace("api-staging.owlhead.ai", "api-stage.owlhead.ai"))
    assert staging_redirect_problems(overlay.replace("api-staging.owlhead.ai", "api.owlhead.ai"), main_tf)
    assert staging_redirect_problems(overlay, main_tf.replace("api_hostname", "api_host"))
