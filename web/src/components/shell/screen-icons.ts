import { Algorithm } from "pixelarticons/react/Algorithm.js";
import { AudioWaveform } from "pixelarticons/react/AudioWaveform.js";
import { AvatarCircle } from "pixelarticons/react/AvatarCircle.js";
import { Bell } from "pixelarticons/react/Bell.js";
import { BellRing } from "pixelarticons/react/BellRing.js";
import { Briefcase } from "pixelarticons/react/Briefcase.js";
import { Bulletlist } from "pixelarticons/react/Bulletlist.js";
import { ChartLine } from "pixelarticons/react/ChartLine.js";
import { CheckDouble } from "pixelarticons/react/CheckDouble.js";
import { Clock } from "pixelarticons/react/Clock.js";
import { Directions } from "pixelarticons/react/Directions.js";
import { Download } from "pixelarticons/react/Download.js";
import { Eye } from "pixelarticons/react/Eye.js";
import { FileText } from "pixelarticons/react/FileText.js";
import { GitCommit } from "pixelarticons/react/GitCommit.js";
import { Grid2x22 } from "pixelarticons/react/Grid2x22.js";
import { Home } from "pixelarticons/react/Home.js";
import { Inbox } from "pixelarticons/react/Inbox.js";
import { Notebook } from "pixelarticons/react/Notebook.js";
import { Pencil } from "pixelarticons/react/Pencil.js";
import { Plug } from "pixelarticons/react/Plug.js";
import { Plus } from "pixelarticons/react/Plus.js";
import { Radio } from "pixelarticons/react/Radio.js";
import { Receipt } from "pixelarticons/react/Receipt.js";
import { Robot } from "pixelarticons/react/Robot.js";
import { Script } from "pixelarticons/react/Script.js";
import { Server } from "pixelarticons/react/Server.js";
import { Terminal } from "pixelarticons/react/Terminal.js";
import { TestTube } from "pixelarticons/react/TestTube.js";
import { Users } from "pixelarticons/react/Users.js";
import type { Icon } from "@/components/icon";
import type { AgentSectionKey } from "@/lib/screens";

/** One icon per screen, shared by the dock, the phone tabs and More. */
export const SCREEN_ICON: Record<string, Icon> = {
  home: Home,
  approvals: Inbox,
  alerts: Bell,
  agents: Robot,
  positions: Briefcase,
  "agents-new": Plus,
  connections: Plug,
  "audit-trace": Directions,
  "audit-decisions": Algorithm,
  "audit-timeline": Clock,
  "audit-surveillance": Eye,
  "audit-export": Download,
  "audit-verify": CheckDouble,
  "settings-policies": Script,
  "settings-members": Users,
  "settings-notifications": BellRing,
  "settings-clients": Terminal,
  "settings-disclosures": FileText,
  "settings-billing": Receipt,
  "settings-deployment": Server,
  "settings-profile": AvatarCircle,
};

export const SECTION_ICON: Record<AgentSectionKey, Icon> = {
  overview: Grid2x22,
  positions: Briefcase,
  orders: Bulletlist,
  decisions: Algorithm,
  approvals: Inbox,
  mandate: Script,
  "mandate/versions": GitCommit,
  "mandate/edit": Pencil,
  prove: TestTube,
  "prove/backtests": ChartLine,
  "prove/paper": Notebook,
  "prove/live": Radio,
  activity: AudioWaveform,
};
