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

/** apca-w3 is licensed for the contrast tests only, and its colorparsley dependency is AGPL-3.0 (web/COLOR.md). */
const APCA_MESSAGE = "APCA is for the contrast tests only: import it from src/test/apca.ts in a test, never in app code.";
const APP_RULES = {
  ...SAFETY_RULES,
  "no-restricted-imports": [
    "error",
    {
      paths: [...SAFETY_RULES["no-restricted-imports"][1].paths, { name: "apca-w3", message: APCA_MESSAGE }, { name: "colorparsley", message: APCA_MESSAGE }],
      patterns: [{ group: ["**/test/apca", "@/test/apca"], message: APCA_MESSAGE }],
    },
  ],
};

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  { files: ["src/**/*.{ts,tsx}"], rules: SAFETY_RULES },
  { files: ["src/**/*.{ts,tsx}"], ignores: ["src/test/**", "src/**/*.test.{ts,tsx}"], rules: APP_RULES },
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    ".next-e2e/**",
    ".next-auth-e2e/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
  ]),
]);

export default eslintConfig;
