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

{{- define "mandate.processes" -}}
{{- (fromJson (.Files.Get "processes.json")).processes | toJson -}}
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
then renders no Deployment of its own, only its pod in that process's templates (DEC-1170).
*/}}
{{- define "mandate.startedElsewhere" -}}
{{- $root := .root -}}
{{- with .p.started_by -}}
{{- $by := . -}}
{{- range (include "mandate.processes" $root | fromJsonArray) -}}
{{- if and (eq .name $by) (index $root.Values.images .image) -}}
true
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
{{- if .p.colocate }}
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
every policy here allows the resolver on port 53 only). The vault has no pod in the local cell and
the journal's Service exposes 5432 alone, so a probe of either would read as filtered at once.
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
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    runAsGroup: 65532
    fsGroup: 65532
    seccompProfile:
      type: RuntimeDefault
  {{- if $p.colocate }}
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
    {{- with .volumes }}
    {{- . | nindent 4 }}
    {{- end }}
{{- end -}}

{{/* One container of process `p`: name, image, command, args, optional extra mounts, and, with
`probe` naming its binary, `exec` liveness and readiness probes on its socket (DEC-994 item 5). */}}
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
  env:
    {{- include "mandate.env" . | nindent 4 }}
  {{- with .probe }}
  livenessProbe:
    exec:
      command: [{{ . | quote }}, "probe", "livez", "--probe", "/tmp/process.sock"]
    initialDelaySeconds: 5
    periodSeconds: 10
    timeoutSeconds: 5
  readinessProbe:
    exec:
      command: [{{ . | quote }}, "probe", "readyz", "--probe", "/tmp/process.sock"]
    periodSeconds: 5
    timeoutSeconds: 5
  {{- end }}
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
its port from values, since a policy cannot select it by pod label.
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

{{/* A `mandate.local/` image only where the values allow it: the local cell's alone. */}}
{{- define "mandate.checkImages" -}}
{{- range $key, $image := .Values.images -}}
{{- if and (hasPrefix "mandate.local/" $image) (not $.Values.localImages) -}}
{{- fail (printf "images.%s is a local build (%s), which only the local cell's values allow" $key $image) -}}
{{- end -}}
{{- end -}}
{{- end -}}
