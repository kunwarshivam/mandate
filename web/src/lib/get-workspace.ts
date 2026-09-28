import { cookies } from "next/headers";
import { buildWorkspace, isScenario } from "@/fixtures/workspace";
import type { Scenario, Workspace } from "@/fixtures/types";
import { SCENARIO_COOKIE, scenariosEnabled } from "./scenario";

export async function getScenario(): Promise<Scenario> {
  if (!scenariosEnabled) return "normal";
  const value = (await cookies()).get(SCENARIO_COOKIE)?.value;
  return isScenario(value) ? value : "normal";
}

export async function getWorkspace(): Promise<Workspace> {
  return buildWorkspace(await getScenario());
}
