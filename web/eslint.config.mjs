import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

/**
 * Safety bans (web/DESIGN.md, "Kumo"). Crimson is the kill switch alone, so Kumo's destructive
 * variants are banned and `KillSwitchButton` is the only way to draw one. The root barrel pulls in
 * every Kumo component (and a syntax highlighter), so imports go per component.
 */
export const SAFETY_RULES = {
  "no-restricted-syntax": [
    "error",
    {
      selector: "JSXAttribute[name.name='variant'][value.value='destructive']",
      message: "Kumo's destructive variant is banned: crimson is for the kill switch only (use KillSwitchButton).",
    },
    {
      selector: "JSXAttribute[name.name='variant'][value.value='secondary-destructive']",
      message: "Kumo's secondary-destructive variant is banned: crimson is for the kill switch only (use KillSwitchButton).",
    },
    {
      selector: "JSXAttribute[name.name='variant'] Literal[value=/^(secondary-)?destructive$/]",
      message: "Kumo's destructive variants are banned: crimson is for the kill switch only (use KillSwitchButton).",
    },
    {
      selector: "JSXOpeningElement[name.name=/^(KillSwitchButton|StopControl|Choice)$/] > JSXAttribute[name.name=/^(disabled|loading)$/]",
      message: "Pause, Stop, and Kill are never disabled or loading; report progress as status text.",
    },
  ],
  "no-restricted-imports": [
    "error",
    {
      paths: [{ name: "@cloudflare/kumo", message: "Import Kumo per component, e.g. @cloudflare/kumo/components/button." }],
    },
  ],
};

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  { files: ["src/**/*.{ts,tsx}"], rules: SAFETY_RULES },
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
  ]),
]);

export default eslintConfig;
