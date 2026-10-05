/**
 * When things happen in each scene of the tour, in seconds from the scene's start. The scenes draw
 * from these (`tour-scenes.js`) and the sound effects land on them (`tour-sound.ts`), so a pop is
 * heard on the frame the window pops.
 */
export const CUES = {
  boot: { drop: 0.15, land: 0.7, letters: 1.05, letterGap: 0.08, bar: [1.9, 4.0], bars: 22, jingle: 4.2 },
  rules: { win: 0.4, typeFrom: 0.6, typeRate: 30, dialog: 4.4, cursor: [5.7, 7.3], press: 7.7 },
  ideas: { win: 0.4, row0: 1.4, rowGap: 1.25 },
  orders: { step0: 0.6, stepGap: 1.2, phone: 4.5, buzz: 5.0, cursor: [5.6, 6.8], press: 6.9, approved: 7.25 },
  record: { win: 0.4, row0: 0.8, rowGap: 0.55, cursor: [5.0, 5.75], click: 5.85, tamper: 6.0, cascadeGap: 0.13 },
  check: { win: 0.4, item0: 0.7, itemGap: 2.1, stop: 2.9, cursor: [3.1, 4.0], press: 4.2 },
  end: { win: 0.2, sting: 0.45, perch0: 1.5, perchGap: 0.16, fade: 6 },
} as const;

/** The rules typed into the notepad, one character every 1 / `CUES.rules.typeRate` seconds, each a keystroke. */
export const RULES_TEXT = "US stocks only.\nNo more than $2,000 on any one order.\nAsk me before buying anything you haven’t held.";

/** The pixel wipe between scenes: how long the old scene takes to fill, and the new one to clear. */
export const WIPE = 0.32;

/** Frames per second. */
export const FPS = 30;
