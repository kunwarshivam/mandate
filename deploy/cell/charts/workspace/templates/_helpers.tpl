{{/*
Every manifest is written as YAML in a named template and emitted as one JSON document through
"mandate.emit", so the rendered chart is machine-readable without a YAML parser: the invariant
tests (deploy/tests/test_cell.py) and the drills (`cargo xtask cell drill`) read it as JSON.
*/}}

{{- define "mandate.emit" -}}
{{- $doc := fromYaml . -}}
{{- if hasKey $doc "Error" -}}
{{- fail (printf "a template produced YAML that does not parse: %s" (get $doc "Error")) -}}
{{- end -}}
{{ toJson $doc }}
{{- end -}}

{{- /* The processes of processes.json; with `api.vaultBinding` the API's egress also names the
vault, so its pod carries the label the vault's policy admits on 8200 and an egress rule to it
alone (DEC-2338). */ -}}
{{- define "mandate.processes" -}}
{{- $all := (fromJson (.Files.Get "processes.json")).processes -}}
{{- $out := list -}}
{{- range $all -}}
{{- if and (eq .name "api") $.Values.api.vaultBinding -}}
{{- $_ := set . "egress" (concat (list (first .egress) "vault") (rest .egress)) -}}
{{- end -}}
{{- $out = append $out . -}}
{{- if and (eq .name "deploy-manager") $.Values.deployManager.researchWorkspace -}}
{{- $out = append $out (include "mandate.researchManager" (dict "root" $ "p" .) | fromJson) -}}
{{- end -}}
{{- if and (eq .name "scheduler") $.Values.deployManager.researchWorkspace -}}
{{- $out = append $out (include "mandate.researchScheduler" (dict "root" $ "p" .) | fromJson) -}}
{{- end -}}
{{- end -}}
{{- $out | toJson -}}
{{- end -}}

{{- /* The local cell's second deployment manager (DEC-2070): the first's process entry under its own
name, so every per-process rule (its network policy, its colocation) holds for it as for the first,
and it signs in as the first's journal role, serving `deployManager.researchWorkspace` alone: the
hosted workspace's ULID with `r` (DEC-2247 item 2). Only a dev or ci cell names it. */ -}}
{{- define "mandate.researchManager" -}}
{{- if not (has .root.Values.deploymentTier (list "dev" "ci")) -}}
{{- fail (printf "deployManager.researchWorkspace runs only in a dev or ci cell, and deploymentTier is %q (DEC-2070)" .root.Values.deploymentTier) -}}
{{- end -}}
{{- $hosted := .root.Values.api.workspace | default "" -}}
{{- if and $hosted (ne .root.Values.deployManager.researchWorkspace (printf "%sr" $hosted)) -}}
{{- fail (printf "deployManager.researchWorkspace is %q, not the hosted workspace %q with r (DEC-2070)" .root.Values.deployManager.researchWorkspace $hosted) -}}
{{- end -}}
{{- $copy := deepCopy .p -}}
{{- $_ := set $copy "name" "deploy-manager-research" -}}
{{- $_ := set $copy "row" "The local cell's second deployment manager, for the research agent's workspace alone (DEC-2070)" -}}
{{- $copy | toJson -}}
{{- end -}}

{{- /* The local cell's second scheduler (DEC-2071): the first's process entry under its own name,
writing the research workspace's clock stream, `clock:<workspace>r`, whose ticks the runtime and
executor the second manager starts read, as the first writes the hosted workspace's. It renders
only beside the second manager, which refuses any tier but dev or ci (DEC-2070). */ -}}
{{- define "mandate.researchScheduler" -}}
{{- $copy := deepCopy .p -}}
{{- $_ := set $copy "name" "scheduler-research" -}}
{{- $_ := set $copy "row" "The local cell's second scheduler, the single writer of the research workspace's clock stream (DEC-2071)" -}}
{{- $args := list -}}
{{- range .p.args -}}
{{- $args = append $args (ternary $.root.Values.deployManager.researchWorkspace . (eq . "{hosted_workspace}")) -}}
{{- end -}}
{{- $_ := set $copy "args" $args -}}
{{- $copy | toJson -}}
{{- end -}}

{{/* A process renders only when its image is named (processes.json "about"). */}}
{{- define "mandate.rendered" -}}
{{- $root := .root -}}
{{- $out := list -}}
{{- range (include "mandate.processes" $root | fromJsonArray) -}}
{{- if and (eq .kind $.kind) (index $root.Values.images .image) -}}
{{- $out = append $out . -}}
{{- end -}}
{{- end -}}
{{- $out | toJson -}}
{{- end -}}

{{/*
"true" when process `p` is started by another process (its `started_by`) whose image is named: it
then renders no Deployment of its own, only its pod in that process's templates (DEC-1170). A
`one_shot` process (the token exchange) is never a standing Deployment, whether or not that process
renders: only it starts one, once (DEC-1175).
*/}}
{{- define "mandate.startedElsewhere" -}}
{{- $root := .root -}}
{{- if .p.one_shot -}}
true
{{- else -}}
{{- with .p.started_by -}}
{{- $by := . -}}
{{- range (include "mandate.processes" $root | fromJsonArray) -}}
{{- if and (eq .name $by) (index $root.Values.images .image) -}}
true
{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "mandate.labels" -}}
app.kubernetes.io/name: mandate-workspace
app.kubernetes.io/instance: {{ .root.Release.Name | quote }}
app.kubernetes.io/component: {{ .p.name | quote }}
mandate.dev/workspace: {{ .root.Values.workspace | quote }}
mandate.dev/environment: {{ .root.Values.environment | quote }}
mandate.dev/process: {{ .p.name | quote }}
{{- if include "mandate.colocated" . }}
mandate.dev/colocate: "true"
{{- end }}
{{- range .p.egress }}
mandate.dev/egress-{{ . }}: "true"
{{- end }}
{{- end -}}

{{- define "mandate.selector" -}}
mandate.dev/workspace: {{ .root.Values.workspace | quote }}
mandate.dev/process: {{ .p.name | quote }}
{{- end -}}

{{/* The executor and the scheduler get requests equal to limits (§3.3). */}}
{{- define "mandate.resources" -}}
limits:
  cpu: {{ .p.resources.limits.cpu | quote }}
  memory: {{ .p.resources.limits.memory | quote }}
requests:
{{- if .p.guaranteed }}
  cpu: {{ .p.resources.limits.cpu | quote }}
  memory: {{ .p.resources.limits.memory | quote }}
{{- else }}
  cpu: {{ .p.resources.requests.cpu | quote }}
  memory: {{ .p.resources.requests.memory | quote }}
{{- end }}
{{- end -}}

{{- define "mandate.containerSecurity" -}}
allowPrivilegeEscalation: false
readOnlyRootFilesystem: true
privileged: false
runAsNonRoot: true
capabilities:
  drop: ["ALL"]
{{- end -}}

{{- define "mandate.roleSecret" -}}
{{ .root.Values.rolePrefix }}{{ .p.db_role | replace "_" "-" }}
{{- end -}}

{{/*
The workspace's shared artifact store (mandate documents, backtest reports): one claim, at one
fixed path. Who may write it is fixed here, by process type, not a value, so no value widens it:
the control runs write ("rw"); the API server only reads, but its pod lays the store out before it
starts ("seed": the pod's volume is writable, the server's own mount is not); the runtime, the
deployment manager and the model gateway, for each agent's research cap (DEC-2272), read ("ro");
every other process has no store at all.
*/}}
{{- define "mandate.artifactsRole" -}}
{{- $access := dict "api" "seed" "control" "rw" "runtime" "ro" "executor" "ro" "deploy-manager" "ro" "deploy-manager-research" "ro" "model-gateway" "ro" "data" "rw" -}}
{{- get $access .p.name -}}
{{- end -}}

{{/*
Whether the claim renders: only with the long-running process whose pod lays it out (the API server). A
claim on a class that binds on first use (local-path does) binds only when a pod uses it, and `helm
--wait` waits for it to; and a reader alone would wait for a layout no writer makes. "true" or
empty.
*/}}
{{- define "mandate.artifactsClaimed" -}}
{{- $root := . -}}
{{- range (include "mandate.rendered" (dict "root" . "kind" "service") | fromJsonArray) -}}
{{- if eq (include "mandate.artifactsRole" (dict "root" $root "p" .)) "seed" -}}true{{- end -}}
{{- end -}}
{{- end -}}

{{/*
Process `p`'s access to the store, "rw", "seed" or "ro", or empty when it has none or no claim renders, so
no pod ever names a claim that is not there.
*/}}
{{- define "mandate.artifactsAccess" -}}
{{- if include "mandate.artifactsClaimed" .root -}}
{{- include "mandate.artifactsRole" . -}}
{{- end -}}
{{- end -}}

{{/*
"true" when process `p`'s pod must share a node with the workspace's other colocated pods: the
processes colocated for latency (their `colocate`, §3.2), and every pod that mounts the artifact
store, whose claim is ReadWriteOnce, so it attaches to one node at a time (DEC-2273). One group, not
two: a reader that had to join both the store's node and its colocated peers' could find them on
different nodes and never schedule. The first such pod goes where the scheduler puts it, since it
matches its own required affinity; every later one, the API server's after a Recreate among them,
follows. So a node's loss takes the whole group down together, until the store moves to a class that
attaches to many nodes or to the object store (E21-40).
*/}}
{{- define "mandate.colocated" -}}
{{- if or .p.colocate (include "mandate.artifactsAccess" .) -}}true{{- end -}}
{{- end -}}

{{- define "mandate.artifactsPath" -}}/var/lib/mandate/artifacts{{- end -}}

{{/*
The store's layout, before the process starts: the API server refuses a root without `sha256/`,
and the readers need `sha256/` and `tmp/` to exist. A writer's or the API server's pod makes both; a reader's pod,
mounted read-only, waits for a writer to have made them, and after two minutes fails rather than
start without them (AGENTS.md rule 3).
*/}}
{{- define "mandate.artifactsLayout" -}}
{{- $path := include "mandate.artifactsPath" . -}}
- name: artifacts-layout
  image: {{ .root.Values.images.postgres | quote }}
  imagePullPolicy: IfNotPresent
  command:
    - bash
    - -c
    {{- if ne (include "mandate.artifactsAccess" .) "ro" }}
    - {{ printf "mkdir -p %s/sha256 %s/tmp" $path $path | quote }}
    {{- else }}
    - {{ printf "for _ in $(seq 1 120); do [ -d %s/sha256 ] && [ -d %s/tmp ] && exit 0; sleep 1; done; echo 'the artifact store has no sha256/ and tmp/ after two minutes: refusing to start' >&2; exit 1" $path $path | quote }}
    {{- end }}
  securityContext:
    {{- include "mandate.containerSecurity" . | nindent 4 }}
  resources:
    limits: {cpu: "50m", memory: "32Mi"}
    requests: {cpu: "50m", memory: "32Mi"}
  volumeMounts:
    - name: artifacts
      mountPath: {{ $path | quote }}
      readOnly: {{ eq (include "mandate.artifactsAccess" .) "ro" }}
{{- end -}}

{{/* The journal DSN carries no password: libpq and sqlx read it from PGPASSFILE (DEC-822 item 4). */}}

{{- define "mandate.env" -}}
{{- $j := .root.Values.journal -}}
{{- if .p.db_role }}
- name: MANDATE_JOURNAL_DSN
  value: {{ printf "postgres://%s@%s:%v/%s?sslmode=verify-full&sslrootcert=/run/secrets/journal-ca/ca.crt" .p.db_role $j.host $j.port $j.database | quote }}
- name: PGPASSFILE
  value: /run/pg/pgpass
{{- end }}
- name: HOME
  value: /tmp
- name: MANDATE_DEPLOYMENT_TIER
  value: {{ .root.Values.deploymentTier | quote }}
{{- if include "mandate.publicTls" . }}
- name: SSL_CERT_FILE
  value: /run/ca-bundle/ca-certificates.crt
{{- end }}
{{- if has .p.name (list "api" "token-exchange") }}
{{- with .root.Values.alpacaOAuth.clientId }}
- name: MANDATE_ALPACA_CLIENT_ID
  value: {{ . | quote }}
{{- end }}
{{- with .root.Values.alpacaOAuth.redirectUri }}
- name: MANDATE_ALPACA_REDIRECT_URI
  value: {{ . | quote }}
{{- end }}
{{- end }}
{{- if and (eq .p.name "token-exchange") (include "mandate.oauthFixtureOn" .root) }}
- name: MANDATE_TOKEN_URL
  value: http://oauth-fixture:8091/alpaca/oauth/token
{{- end }}
{{- if and (eq .p.name "api") (include "mandate.oauthFixtureOn" .root) }}
- name: MANDATE_ALPACA_AUTHORIZE_URL
  value: http://oauth-fixture:8091/alpaca/oauth/authorize
{{- end }}
{{- if and (eq .p.name "executor") (include "mandate.oauthFixtureOn" .root) }}
- name: MANDATE_ALPACA_ACCOUNT_URL
  value: http://oauth-fixture:8091/alpaca/v2/account
{{- end }}
{{- /* The model gateway's workspace and deployment, required with its image, and its two monthly
budgets, rendered only when named, so an unnamed one stops its start with its code (DEC-2326). */}}
{{- if eq .p.name "model-gateway" }}
{{- $g := .root.Values.modelGateway }}
{{- if and (or $g.workspaceMonthlyUsd $g.deploymentMonthlyUsd) (not .root.Values.localImages) }}
{{- fail "modelGateway's monthly budgets are named only by the local cell's values (localImages), over the recorded bundle; a hosted cell's budgets are the founder's (DEC-2326 item 2, DEC-2396)" }}
{{- end }}
- name: MANDATE_GATEWAY_WORKSPACE
  value: {{ required "modelGateway.workspace, the one workspace the model gateway serves, is required with images.model-gateway" $g.workspace | quote }}
- name: MANDATE_GATEWAY_DEPLOYMENT
  value: {{ required "modelGateway.deployment, whose meter stream the model gateway writes, is required with images.model-gateway" $g.deployment | quote }}
{{- with $g.workspaceMonthlyUsd }}
- name: MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD
  value: {{ . | quote }}
{{- end }}
{{- with $g.deploymentMonthlyUsd }}
- name: MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD
  value: {{ . | quote }}
{{- end }}
{{- end }}
{{- if include "mandate.artifactsAccess" . }}
- name: {{ ternary "MANDATE_API_ARTIFACTS" "MANDATE_ARTIFACTS" (eq .p.name "api") }}
  value: {{ include "mandate.artifactsPath" . | quote }}
{{- end }}
{{- /* The stored bars' root the runtime's dry run and the executor's binding gate read their
liquidity figures from (DEC-2274), only where the data process stores them. */}}
{{- if and (has .p.name (list "runtime" "executor")) .root.Values.images.data (include "mandate.artifactsAccess" .) }}
- name: MANDATE_BARS
  value: {{ printf "%s/datasets/cell-recorded" (include "mandate.artifactsPath" .) | quote }}
{{- end }}
{{- end -}}

{{- define "mandate.mounts" -}}
- name: tmp
  mountPath: /tmp
{{- if .p.db_role }}
- name: pg
  mountPath: /run/pg
  readOnly: true
- name: journal-ca
  mountPath: /run/secrets/journal-ca
  readOnly: true
{{- end }}
{{- with include "mandate.artifactsAccess" . }}
- name: artifacts
  mountPath: {{ include "mandate.artifactsPath" $ | quote }}
  readOnly: {{ ne . "rw" }}
{{- end }}
{{- if include "mandate.vaultToken" . }}
- name: vault-token
  mountPath: /run/mandate/vault-token
  readOnly: true
{{- end }}
{{- if include "mandate.vaultCa" . }}
- name: vault-ca
  mountPath: /run/mandate/vault
  readOnly: true
{{- end }}
{{- if include "mandate.publicTls" . }}
- name: ca-bundle
  mountPath: /run/ca-bundle
  readOnly: true
{{- end }}
{{- end -}}

{{/*
The egress gate, every pod's first container. A network-policy controller may program a new pod's
filter only seconds after the pod starts (k3s's does), and until then the pod reaches anything. So
nothing of the process runs until the pod's own egress is shown filtered: the Kubernetes API server,
which no policy here allows, must stop answering. Still answering after a minute, the pod fails and
never starts (AGENTS.md rule 3). Residual: an API server that is down at the same moment reads as
filtered.
The deployment manager's policy allows the API server (`kube-api`, DEC-1172), so its gate probes a
destination that policy denies and that answers whenever nothing filters: the cluster resolver's
metrics port, 9153, at the address in the pod's resolv.conf (CoreDNS's `kube-dns` Service serves it;
every policy here allows the resolver on port 53 only). The vault admits only its own processes on
8200 (its chart's policy) and the journal's Service exposes 5432 alone, so a probe of either could
read as filtered whether or not the pod's own egress is.
Residual: a resolver that serves no metrics port reads as filtered at once.
*/}}
{{- define "mandate.egressGate" -}}
- name: egress-gate
  image: {{ .root.Values.images.postgres | quote }}
  imagePullPolicy: IfNotPresent
  command:
    - bash
    - -c
    {{- if has "kube-api" .p.egress }}
    - |
      dns=$(awk '$1 == "nameserver" { print $2; exit }' /etc/resolv.conf)
      [ -n "$dns" ] || { echo "no cluster resolver address: refusing to start" >&2; exit 1; }
      for _ in $(seq 1 60); do
        if timeout 2 bash -c "exec 3<>/dev/tcp/$dns/9153" 2>/dev/null; then
          sleep 1
        else
          echo "egress filtered"
          exit 0
        fi
      done
      echo "egress still open after a minute: refusing to start" >&2
      exit 1
    {{- else }}
    - |
      [ -n "${KUBERNETES_SERVICE_HOST:-}" ] && [ -n "${KUBERNETES_SERVICE_PORT:-}" ] || { echo "no API server address: refusing to start" >&2; exit 1; }
      for _ in $(seq 1 60); do
        if timeout 2 bash -c "exec 3<>/dev/tcp/$KUBERNETES_SERVICE_HOST/$KUBERNETES_SERVICE_PORT" 2>/dev/null; then
          sleep 1
        else
          echo "egress filtered"
          exit 0
        fi
      done
      echo "egress still open after a minute: refusing to start" >&2
      exit 1
    {{- end }}
  securityContext:
    {{- include "mandate.containerSecurity" . | nindent 4 }}
  resources:
    limits: {cpu: "50m", memory: "32Mi"}
    requests: {cpu: "50m", memory: "32Mi"}
{{- end -}}

{{/*
The pod around a process's containers. Only the deployment manager's passes `serviceAccount`, the
one pod given an API token (DEC-1172); every other pod mounts none. Secrets are files: the role's pgpass is mounted group-
readable for fsGroup, and libpq and sqlx refuse a pgpass any group can read, so an init container
copies it to a memory-backed directory as 0600 under the process's own user.
*/}}
{{- define "mandate.pod" -}}
{{- $root := .root -}}
{{- $p := .p -}}
metadata:
  labels:
    {{- include "mandate.labels" . | nindent 4 }}
  {{- with .annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
spec:
  {{- with .serviceAccount }}
  serviceAccountName: {{ . | quote }}
  automountServiceAccountToken: true
  {{- else }}
  automountServiceAccountToken: false
  {{- end }}
  enableServiceLinks: false
  restartPolicy: {{ .restartPolicy }}
  {{- with .grace }}
  terminationGracePeriodSeconds: {{ . }}
  {{- end }}
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    runAsGroup: 65532
    fsGroup: 65532
    seccompProfile:
      type: RuntimeDefault
  {{- if include "mandate.colocated" . }}
  affinity:
    podAffinity:
      requiredDuringSchedulingIgnoredDuringExecution:
        - topologyKey: kubernetes.io/hostname
          labelSelector:
            matchLabels:
              mandate.dev/workspace: {{ $root.Values.workspace | quote }}
              mandate.dev/colocate: "true"
  {{- end }}
  initContainers:
    {{- include "mandate.egressGate" . | nindent 4 }}
    {{- if $p.db_role }}
    - name: credentials
      image: {{ $root.Values.images.postgres | quote }}
      imagePullPolicy: IfNotPresent
      command: ["install", "-m", "0600", "/run/secrets/journal-role/pgpass", "/run/pg/pgpass"]
      securityContext:
        {{- include "mandate.containerSecurity" . | nindent 8 }}
      resources:
        limits: {cpu: "50m", memory: "32Mi"}
        requests: {cpu: "50m", memory: "32Mi"}
      volumeMounts:
        - name: pg
          mountPath: /run/pg
        - name: journal-role
          mountPath: /run/secrets/journal-role
          readOnly: true
    {{- end }}
    {{- if include "mandate.artifactsAccess" . }}
    {{- include "mandate.artifactsLayout" . | nindent 4 }}
    {{- end }}
    {{- if include "mandate.publicTls" . }}
    - name: ca-bundle
      image: {{ $root.Values.images.postgres | quote }}
      imagePullPolicy: IfNotPresent
      command: ["cp", "/etc/ssl/certs/ca-certificates.crt", "/run/ca-bundle/ca-certificates.crt"]
      securityContext:
        {{- include "mandate.containerSecurity" . | nindent 8 }}
      resources:
        limits: {cpu: "50m", memory: "32Mi"}
        requests: {cpu: "50m", memory: "32Mi"}
      volumeMounts:
        - name: ca-bundle
          mountPath: /run/ca-bundle
    {{- end }}
    {{- with .initContainers }}
    {{- . | nindent 4 }}
    {{- end }}
  containers:
    {{- .containers | nindent 4 }}
  volumes:
    - name: tmp
      emptyDir:
        medium: Memory
        sizeLimit: 16Mi
    - name: work
      emptyDir:
        sizeLimit: 64Mi
    {{- if include "mandate.publicTls" . }}
    - name: ca-bundle
      emptyDir:
        sizeLimit: 1Mi
    {{- end }}
    {{- if $p.db_role }}
    - name: pg
      emptyDir:
        medium: Memory
        sizeLimit: 1Mi
    - name: journal-role
      secret:
        secretName: {{ include "mandate.roleSecret" . | quote }}
        defaultMode: 288
        items:
          - key: pgpass
            path: pgpass
    - name: journal-ca
      secret:
        secretName: {{ $root.Values.journal.ca | quote }}
        defaultMode: 292
        items:
          - key: ca.crt
            path: ca.crt
    {{- end }}
    {{- if include "mandate.artifactsAccess" . }}
    - name: artifacts
      persistentVolumeClaim:
        claimName: artifacts
        readOnly: {{ eq (include "mandate.artifactsAccess" .) "ro" }}
    {{- end }}
    {{- if include "mandate.vaultToken" . }}
    - name: vault-token
      projected:
        defaultMode: 292
        sources:
          - serviceAccountToken:
              audience: mandate-vault
              expirationSeconds: 600
              path: token
          - downwardAPI:
              items:
                - path: namespace
                  fieldRef:
                    fieldPath: metadata.namespace
    {{- end }}
    {{- if include "mandate.vaultCa" . }}
    - name: vault-ca
      configMap:
        name: vault-ca
        defaultMode: 292
        items:
          - key: ca.crt
            path: ca.crt
    {{- end }}
    {{- with .volumes }}
    {{- . | nindent 4 }}
    {{- end }}
{{- end -}}

{{/*
A service's launch from processes.json (DEC-1013): its `args`, and its `probe` asked `livez` and
`readyz` (DEC-994 item 5), with `{workspace}`, `{hosted_workspace}` (`api.workspace`, the workspace
the deployment manager hosts and whose clock stream the scheduler writes, E21-29), and the `runtime`
values' `{agent}`, `{connection}` and `{owner}`, and `{data_instruments}`, the `data.instruments`
list as `<symbol>:<assetId>:<anchor>`, comma-separated, filled in. A placeholder whose value is
unset fails the render rather than starting a process for no deployment, and so does a symbol
listed twice.
*/}}
{{/*
`data.instruments` as the data process's `--instruments`: `<symbol>:<assetId>:<anchor>` for each,
comma-separated, in the values' order. An empty list, a missing field, or a symbol listed twice
fails the render.
*/}}
{{- define "mandate.dataInstruments" -}}
{{- $all := .Values.data.instruments -}}
{{- if not $all }}{{ fail "data.instruments, the instruments whose bars the data process stores, is required with images.data" }}{{ end -}}
{{- $seen := dict -}}
{{- $out := list -}}
{{- range $all -}}
{{- $symbol := required "data.instruments[].symbol is required with images.data" .symbol -}}
{{- if hasKey $seen $symbol }}{{ fail (printf "data.instruments lists %s twice" $symbol) }}{{ end -}}
{{- $_ := set $seen $symbol true -}}
{{- $asset := required "data.instruments[].assetId, the asset id the instrument's closes object names, is required with images.data" .assetId -}}
{{- $anchor := required "data.instruments[].anchor, the recorded series' `<YYYY-MM-DD>=<close>`, is required with images.data" .anchor -}}
{{- $out = append $out (printf "%s:%s:%s" $symbol $asset $anchor) -}}
{{- end -}}
{{- join "," $out -}}
{{- end -}}

{{- define "mandate.fill" -}}
{{- $v := .root.Values -}}
{{- $out := list -}}
{{- range .words -}}
{{- $w := . -}}
{{- if contains "{workspace}" $w }}{{ $w = replace "{workspace}" $v.workspace $w }}{{ end -}}
{{- if contains "{hosted_workspace}" $w }}{{ $w = replace "{hosted_workspace}" (required "api.workspace, the hosted workspace's ULID whose clock stream the scheduler writes, is required with images.scheduler" $v.api.workspace) $w }}{{ end -}}
{{- if contains "{data_instruments}" $w }}{{ $w = replace "{data_instruments}" (include "mandate.dataInstruments" $.root) $w }}{{ end -}}
{{- if contains "{agent}" $w }}{{ $w = replace "{agent}" (required "runtime.agent is required once images.runtime is named" $v.runtime.agent) $w }}{{ end -}}
{{- if contains "{connection}" $w }}{{ $w = replace "{connection}" (required "runtime.connection is required once images.runtime is named" $v.runtime.connection) $w }}{{ end -}}
{{- if contains "{owner}" $w }}{{ $w = replace "{owner}" (required "runtime.owner is required once images.runtime is named" $v.runtime.owner) $w }}{{ end -}}
{{- if contains "{probe}" $w }}{{ $w = replace "{probe}" $.probe $w }}{{ end -}}
{{- if contains "{vault}" $w }}{{ $w = replace "{vault}" (include "mandate.vaultAddress" $.root) $w }}{{ end -}}
{{- $out = append $out $w -}}
{{- end -}}
{{- $out | toJson -}}
{{- end -}}

{{/* One container of process `p`: name, image, command, args, probes, and optional extra mounts. */}}
{{- define "mandate.container" -}}
- name: {{ .name | quote }}
  image: {{ index .root.Values.images .p.image | quote }}
  imagePullPolicy: IfNotPresent
  {{- with .command }}
  command: {{ toJson . }}
  {{- end }}
  {{- with .args }}
  args: {{ toJson . }}
  {{- end }}
  {{- with .probe }}
  readinessProbe:
    exec:
      command: {{ toJson (index . "readyz") }}
    periodSeconds: 5
    failureThreshold: 3
  livenessProbe:
    exec:
      command: {{ toJson (index . "livez") }}
    periodSeconds: 10
    failureThreshold: 3
  {{- end }}
  env:
    {{- include "mandate.env" . | nindent 4 }}
  securityContext:
    {{- include "mandate.containerSecurity" . | nindent 4 }}
  resources:
    {{- include "mandate.resources" . | nindent 4 }}
  volumeMounts:
    {{- include "mandate.mounts" . | nindent 4 }}
    {{- with .mounts }}
    {{- . | nindent 4 }}
    {{- end }}
{{- end -}}

{{/*
The egress allowed to process `p`: DNS when it has any target, each in-cluster target by pod
label and port, and each external target's CIDRs from values (an empty list allows nothing). The
API server (`kube-api`, the deployment manager's alone) is an external-style target: its CIDRs and
its port from values, since a policy cannot select it by pod label. In a dev or ci cell running the
fixture provider, the executor also reaches the fixture's pod on 8091 for its account read
(DEC-1178 item 4), and nothing else beside its own targets.
*/}}
{{- define "mandate.egressRules" -}}
{{- $root := .root -}}
{{- $v := $root.Values -}}
{{- if .p.egress }}
- to:
    - namespaceSelector:
        matchLabels:
          kubernetes.io/metadata.name: {{ $v.egress.dns.namespace | quote }}
      podSelector:
        matchLabels:
          {{- toYaml $v.egress.dns.podSelector | nindent 10 }}
  ports:
    - {protocol: UDP, port: 53}
    - {protocol: TCP, port: 53}
{{- end }}
{{- range .p.egress }}
{{- if eq . "postgres" }}
- to:
    - podSelector:
        matchLabels:
          {{- toYaml $v.journal.podSelector | nindent 10 }}
  ports:
    - {protocol: TCP, port: {{ $v.journal.port }}}
{{- else if eq . "vault" }}
- to:
    - podSelector:
        matchLabels:
          mandate.dev/workspace: {{ $v.workspace | quote }}
          mandate.dev/process: vault
  ports:
    - {protocol: TCP, port: 8200}
{{- else if eq . "kube-api" }}
{{- with (index $v.egress "kube-api") }}
- to:
    {{- range . }}
    - ipBlock:
        cidr: {{ . | quote }}
    {{- end }}
  ports:
    - {protocol: TCP, port: {{ $v.egress.kubeApiPort }}}
{{- end }}
{{- else if and (eq . "token-host") (include "mandate.oauthFixtureOn" $root) }}
{{- if index $v.egress "token-host" }}
{{- fail "egress.token-host names CIDRs, but with oauthFixture.enabled a dev or ci cell's token exchange reaches the fixture pod alone (DEC-1176 item 4)" }}
{{- end }}
- to:
    - podSelector:
        matchLabels:
          mandate.dev/workspace: {{ $v.workspace | quote }}
          mandate.dev/process: oauth-fixture
  ports:
    - {protocol: TCP, port: 8091}
{{- else if eq . "model-gateway" }}
- to:
    - podSelector:
        matchLabels:
          mandate.dev/workspace: {{ $v.workspace | quote }}
          mandate.dev/process: model-gateway
  ports:
    - {protocol: TCP, port: 8080}
{{- else }}
{{- $cidrs := index $v.egress . }}
{{- if $cidrs }}
- to:
    {{- range $cidrs }}
    - ipBlock:
        cidr: {{ . | quote }}
    {{- end }}
  ports:
    {{- if eq . "time" }}
    - {protocol: UDP, port: 123}
    {{- else }}
    - {protocol: TCP, port: 443}
    {{- end }}
{{- end }}
{{- end }}
{{- end }}
{{- if and (eq .p.name "executor") (include "mandate.oauthFixtureOn" $root) }}
- to:
    - podSelector:
        matchLabels:
          mandate.dev/workspace: {{ $v.workspace | quote }}
          mandate.dev/process: oauth-fixture
  ports:
    - {protocol: TCP, port: 8091}
{{- end }}
{{- end -}}

{{- define "mandate.egressPolicy" -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ printf "egress-%s" .p.name | quote }}
  labels:
    {{- include "mandate.labels" . | nindent 4 }}
  {{- with .annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
spec:
  podSelector:
    matchLabels:
      {{- include "mandate.selector" . | nindent 6 }}
  policyTypes: ["Ingress", "Egress"]
  egress:
    {{- include "mandate.egressRules" . | nindent 4 }}
{{- end -}}

{{/* The one inbound a process accepts: from the processes that name it as an egress target. */}}
{{- define "mandate.ingressPolicy" -}}
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: {{ printf "ingress-%s" .p.name | quote }}
  labels:
    {{- include "mandate.labels" . | nindent 4 }}
spec:
  podSelector:
    matchLabels:
      {{- include "mandate.selector" . | nindent 6 }}
  policyTypes: ["Ingress"]
  ingress:
    - from:
        - podSelector:
            matchLabels:
              mandate.dev/workspace: {{ .root.Values.workspace | quote }}
              mandate.dev/egress-{{ .p.name }}: "true"
      ports:
        - {protocol: TCP, port: {{ .port }}}
{{- end -}}

{{- /* "true" when the fixture OAuth provider renders (DEC-1176 item 2): `oauthFixture.enabled`,
which fails the render in any tier but dev or ci, whether or not `images.stub-issuer` is named,
and needs that image in dev and ci. Called with the chart's root. */ -}}
{{- define "mandate.oauthFixtureOn" -}}
{{- if .Values.oauthFixture.enabled -}}
{{- if not (has .Values.deploymentTier (list "dev" "ci")) -}}
{{- fail (printf "oauthFixture.enabled runs only in a dev or ci cell, and deploymentTier is %q (DEC-1176)" .Values.deploymentTier) -}}
{{- end -}}
{{- $_ := required "images.stub-issuer is required once oauthFixture.enabled is set (DEC-1176)" (index .Values.images "stub-issuer") -}}
true
{{- end -}}
{{- end -}}

{{/* A `mandate.local/` image only where the values allow it: the local cell's alone. */}}
{{- define "mandate.checkImages" -}}
{{- range $key, $image := .Values.images -}}
{{- if and (hasPrefix "mandate.local/" $image) (not $.Values.localImages) -}}
{{- fail (printf "images.%s is a local build (%s), which only the local cell's values allow" $key $image) -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- /* The executor's broker flags (DEC-2306 item 1), as JSON: none for the fixture broker, and for
`alpaca-paper` the paper host's, the vault's address, and the directory the vault's CA is mounted
in. */ -}}
{{- /* Whether process `p` needs the public CA bundle (DEC-2371): its egress names a host
reached over public TLS, an identity provider, a broker, market data, the OAuth token host, a model
provider, object storage or a timestamping service. Its image is a bare binary with no roots, so
its HTTP client could not even be built. The API gets its own bundle in its own pod (`api.yaml`).
*/ -}}
{{- define "mandate.publicTls" -}}
{{- if ne .p.name "api" -}}
{{- range (.p.egress | default list) -}}
{{- if has . (list "identity-provider" "broker" "market-data" "token-host" "model-providers" "object-storage" "timestamping") -}}true{{- end -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- /* Whether process `p` trusts the vault: the vault chart's CA certificate alone, the
ConfigMap `vault-ca`, read-only at /run/mandate/vault (DEC-2332 item 5): a `vault_ca` process's,
the executor's only when its broker reaches the vault. */ -}}
{{- define "mandate.vaultCa" -}}
{{- if and .p.vault_ca (or (ne .p.name "executor") (eq .root.Values.executor.broker "alpaca-paper")) -}}true{{- end -}}
{{- end -}}

{{- /* Whether process `p` mounts a projected ServiceAccount token for the vault (DEC-2060): a
`vault_token` process's, only as a pod the deployment manager starts, which runs as its
connection's own account (DEC-2336), and the executor's only when its broker reaches the vault. A
process the chart runs as a Deployment of its own runs as `default`, which no vault role names, and
mounts none. */ -}}
{{- define "mandate.vaultToken" -}}
{{- if and .p.vault_token (include "mandate.startedElsewhere" .) (or (ne .p.name "executor") (eq .root.Values.executor.broker "alpaca-paper")) -}}true{{- end -}}
{{- end -}}

{{- /* The vault's address: the executor's `vaultAddress` where it is set, else this namespace's
vault Service, which the vault chart's certificate names. */ -}}
{{- define "mandate.vaultAddress" -}}
{{- .Values.executor.vaultAddress | default (printf "https://vault.%s.svc:8200" .Release.Namespace) -}}
{{- end -}}

{{- define "mandate.executorBroker" -}}
{{- $e := .Values.executor -}}
{{- if eq $e.broker "alpaca-paper" -}}
{{- list "--broker" "alpaca-paper" "--vault-address" $e.vaultAddress "--vault-dir" "/run/mandate/vault" | toJson -}}
{{- else -}}
[]
{{- end -}}
{{- end -}}


{{- /*
A process's AppRole and the vault's CA as owner-only files (DEC-2338, DEC-2630 item 6), for the
process whose vault role is `role`: the Secret the vault's provisioning makes for it,
`vault-approle-<role>` (`role-id`, `secret-id`), and the ConfigMap `vault-ca`, projected read-only
into one source directory, mounted 0440 for the pod's fsGroup, and copied by an init container into
a memory-backed directory, `/run/mandate/vault`, as `<role>.role-id`, `<role>.secret-id` and
`ca.crt` at 0600 with nothing else in it, since `mandate_vault`'s reader refuses a file any group or
other can read. The process mounts that directory read-only and reads them there as files, never
from its environment; it never mounts the Secret itself.
*/ -}}
{{- define "mandate.vaultCopy" -}}
{{- $sources := printf "/run/secrets/vault-%s" .role -}}
- name: vault-credentials
  image: {{ .root.Values.images.postgres | quote }}
  imagePullPolicy: IfNotPresent
  command: ["install", "-m", "0600", {{ printf "%s/%s.role-id" $sources .role | quote }}, {{ printf "%s/%s.secret-id" $sources .role | quote }}, {{ printf "%s/ca.crt" $sources | quote }}, "/run/mandate/vault/"]
  securityContext:
    {{- include "mandate.containerSecurity" . | nindent 4 }}
  resources:
    limits: {cpu: "50m", memory: "32Mi"}
    requests: {cpu: "50m", memory: "32Mi"}
  volumeMounts:
    - {name: vault, mountPath: /run/mandate/vault}
    - {name: vault-sources, mountPath: {{ $sources }}, readOnly: true}
{{- end -}}
{{- define "mandate.vaultVolumes" -}}
- name: vault
  emptyDir: {medium: Memory, sizeLimit: 1Mi}
- name: vault-sources
  projected:
    defaultMode: 288
    sources:
      - secret:
          name: {{ printf "vault-approle-%s" .role }}
          items:
            - {key: role-id, path: {{ printf "%s.role-id" .role }}}
            - {key: secret-id, path: {{ printf "%s.secret-id" .role }}}
      - configMap:
          name: vault-ca
          items:
            - {key: ca.crt, path: ca.crt}
{{- end -}}

{{- define "mandate.modelGatewayService" -}}
apiVersion: v1
kind: Service
metadata:
  name: model-gateway
  labels:
    {{- include "mandate.labels" . | nindent 4 }}
spec:
  type: ClusterIP
  selector:
    {{- include "mandate.selector" . | nindent 4 }}
  ports:
    - {name: calls, port: 8080, targetPort: 8080, protocol: TCP}
{{- end -}}
