import type { OwlMood } from "@/components/domain/owl-sprite";

/**
 * Sample agents for the landing page's owls; each ID draws its own face. No React here, so the
 * tour's renderer (`scripts/render-tour.ts`) casts the same five owls the perch shows.
 */
export const PERCH: Array<{ seed: string; mood: OwlMood }> = [
  { seed: "agt_01JB3K8Y4N7QW2M6R9T5V0XZAC", mood: "awake" },
  { seed: "agt_01JB3K9P2H6SD4F8G1E3W7XYZB", mood: "awake" },
  { seed: "agt_01JB3KH9ZT1W3E5R7Y2U4I6O8P", mood: "asleep" },
  { seed: "agt_01JB3KAQ5R8TV2N4M6P9S1W3XD", mood: "awake" },
  { seed: "agt_01JB3KD7XC2M9QW4E6R8T0Y1ZN", mood: "awake" },
];
