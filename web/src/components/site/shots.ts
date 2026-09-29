/**
 * The app screenshots on the landing page (DEC-212), made by `scripts/site-shots.mjs` into
 * `public/site/<name>-light.png` and `<name>-dark.png` at 2x. Sizes are CSS pixels, half the files';
 * `shots.test.ts` reads each file and fails when a size here is wrong.
 */
export type ShotName = "hero-desktop" | "hero-phone" | "step-mandate" | "step-gate" | "step-gate-phone" | "step-approve" | "step-stop";

export type Shot = { width: number; height: number; alt: string };

export const SHOTS: Record<ShotName, Shot> = {
  "hero-desktop": {
    width: 1100,
    height: 720,
    alt: "An agent's overview in Owlhead on a computer: its equity on a chart with the daily loss limit drawn as a line, its mandate beside it with how much room each limit has left, and a Stop this agent button.",
  },
  "hero-phone": {
    width: 390,
    height: 844,
    alt: "Owlhead's home screen on a phone: a request from Agent 2 to buy 2 XYZ at a limit of $141.30 waiting for you, the account's equity, and Stop at the top.",
  },
  "step-mandate": {
    width: 320,
    height: 591,
    alt: "An agent's mandate: a daily loss limit of $9,751.00, and positions of $1,129.84 of $1,500.00 and $488.25 of $1,500.00, each with the room it has left.",
  },
  "step-gate": {
    width: 572,
    height: 538,
    alt: "Recent gate decisions. Buy 0.02 BTC/USD is not allowed because orders are at most $1,000.00. Buy 2 XYZ is allowed and asks you for approval.",
  },
  "step-gate-phone": {
    width: 350,
    height: 512,
    alt: "Recent gate decisions on a phone. Buy 0.02 BTC/USD is not allowed because orders are at most $1,000.00. Buy 2 XYZ is allowed and asks you for approval.",
  },
  "step-approve": {
    width: 390,
    height: 844,
    alt: "An approval request on a phone: Agent 2 proposes to buy 2 XYZ at a limit of $141.30, says why you are asked, and offers Approve and Skip side by side. If you do nothing, it is skipped.",
  },
  "step-stop": {
    width: 448,
    height: 720,
    alt: "The Stop sheet: pause this agent, its kill switch, or pause every agent on the account.",
  },
};

export const SHOT_DIR = "/site";

export function shotSrc(name: ShotName, mode: "light" | "dark"): string {
  return `${SHOT_DIR}/${name}-${mode}.png`;
}
