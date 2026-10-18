{{/* As the workspace chart's "mandate.emit": every manifest is emitted as one JSON document. */}}
{{- define "store.emit" -}}
{{- $doc := fromYaml . -}}
{{- if hasKey $doc "Error" -}}
{{- fail (printf "a template produced YAML that does not parse: %s" (get $doc "Error")) -}}
{{- end -}}
{{ toJson $doc }}
{{- end -}}

{{- define "store.labels" -}}
app.kubernetes.io/name: mandate-archive-store
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- end -}}
