import { Chart } from "pixelarticons/react/Chart.js";
import { Home } from "pixelarticons/react/Home.js";
import { Plus } from "pixelarticons/react/Plus.js";
import { SettingsCog } from "pixelarticons/react/SettingsCog.js";
import type { Icon } from "@/components/icon";

/** One icon per screen, shared by the dock, the phone tabs and More. */
export const SCREEN_ICON: Record<string, Icon> = {
  home: Home,
  portfolio: Chart,
  new: Plus,
  settings: SettingsCog,
};
