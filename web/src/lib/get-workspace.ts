import { cookies } from "next/headers";
import { buildWorkspace, isScenario } from "@/fixtures/workspace";
import type { Scenario, Workspace } from "@/fixtures/types";
import { CVD_COOKIE, colourBlindEnabled } from "./colour-pref";
import { SCENARIO_COOKIE, scenariosEnabled } from "./scenario";
import { THEME_COOKIE, type ThemePref, isThemePref } from "./theme";

export async function getScenario(): Promise<Scenario> {
  if (!scenariosEnabled) return "normal";
  const value = (await cookies()).get(SCENARIO_COOKIE)?.value;
  return isScenario(value) ? value : "normal";
}

export async function getColourBlind(): Promise<boolean> {
  if (!colourBlindEnabled) return false;
  return (await cookies()).get(CVD_COOKIE)?.value === "on";
}

export async function getThemePref(): Promise<ThemePref> {
  const value = (await cookies()).get(THEME_COOKIE)?.value;
  return isThemePref(value) ? value : "system";
}

export async function getWorkspace(): Promise<Workspace> {
  return buildWorkspace(await getScenario());
}
