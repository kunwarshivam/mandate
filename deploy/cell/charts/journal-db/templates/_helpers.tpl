{{/* As the workspace chart's "mandate.emit": every manifest is emitted as one JSON document. */}}
{{- define "journal.emit" -}}
{{- $doc := fromYaml . -}}
{{- if hasKey $doc "Error" -}}
{{- fail (printf "a template produced YAML that does not parse: %s" (get $doc "Error")) -}}
{{- end -}}
{{ toJson $doc }}
{{- end -}}

{{/* Every process type's database role, from the workspace chart's processes.json. */}}
{{- define "journal.roles" -}}
{{- $roles := list -}}
{{- range (fromJson (.Files.Get "files/processes.json")).processes -}}
{{- if and .db_role (ne .db_role "mandate_migrator") (not (has .db_role $roles)) -}}
{{- $roles = append $roles .db_role -}}
{{- end -}}
{{- end -}}
{{- $roles | toJson -}}
{{- end -}}

{{- define "journal.labels" -}}
app.kubernetes.io/name: mandate-journal-db
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- end -}}
