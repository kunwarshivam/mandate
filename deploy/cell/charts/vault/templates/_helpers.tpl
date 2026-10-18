{{/* As the workspace chart's "mandate.emit": every manifest is emitted as one JSON document. */}}
{{- define "vault.emit" -}}
{{- $doc := fromYaml . -}}
{{- if hasKey $doc "Error" -}}
{{- fail (printf "a template produced YAML that does not parse: %s" (get $doc "Error")) -}}
{{- end -}}
{{ toJson $doc }}
{{- end -}}

{{/* The labels the workspace chart's vault egress selects (`mandate.dev/process: vault`). */}}
{{- define "vault.selector" -}}
mandate.dev/workspace: {{ .Values.workspace | quote }}
mandate.dev/process: vault
{{- end -}}

{{- define "vault.labels" -}}
app.kubernetes.io/name: mandate-vault-server
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{ include "vault.selector" . }}
{{- end -}}

{{/* Every name the server certificate holds: the Service in the namespace, and the loopback
address the operator's `bao` in the pod speaks to. */}}
{{- define "vault.dnsNames" -}}
{{- $ns := .Release.Namespace -}}
{{- toJson (list "vault" (printf "vault.%s" $ns) (printf "vault.%s.svc" $ns) (printf "vault.%s.svc.cluster.local" $ns) "localhost") -}}
{{- end -}}
