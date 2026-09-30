/**
 * Tour.mp4, scene by scene. The video is drawn from these scenes by `scripts/render-tour.ts`; its
 * captions and the Media Player's transcript are these lines too, so the three never drift apart.
 * Every line is something the home page says.
 */
export type Scene = { id: "boot" | "rules" | "ideas" | "orders" | "record" | "check" | "end"; start: number; end: number; caption: string };

export const TOUR: Scene[] = [
  { id: "boot", start: 0, end: 5, caption: "Owlhead is a trading agent for your own brokerage account." },
  { id: "rules", start: 5, end: 15, caption: "You write its rules in plain English. Before you confirm, it shows you what it understood, in dollars." },
  { id: "ideas", start: 15, end: 23, caption: "It looks for ideas, and writes each one down with its reason and what would prove it wrong." },
  { id: "orders", start: 23, end: 33, caption: "Every order is sized and checked against your rules. If your rules say to ask, it waits on your phone until you approve it." },
  { id: "record", start: 33, end: 42, caption: "Before any order goes out, it writes down what it read, what it concluded and who approved it. Edit a line and the chain stops matching." },
  { id: "check", start: 42, end: 51, caption: "It trades on paper first, with simulated money. One Stop button halts every agent. It can't take money out of your account." },
  { id: "end", start: 51, end: 58, caption: "Owlhead is in private beta. Ask for a place at owlhead.ai." },
];

export const TOUR_SECONDS = TOUR.at(-1)!.end;

export const TOUR_VIDEO = { src: "/video/owlhead-tour.mp4", poster: "/video/owlhead-tour.jpg", captions: "/video/owlhead-tour.vtt" } as const;

const stamp = (s: number) => `00:${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}.000`;

/** The captions as WebVTT. */
export function tourVtt(): string {
  return ["WEBVTT", "", ...TOUR.flatMap((s, i) => [String(i + 1), `${stamp(s.start)} --> ${stamp(s.end)}`, s.caption, ""])].join("\n");
}
