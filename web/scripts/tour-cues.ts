/**
 * When things happen in each scene of the tour, in seconds from the scene's start. The scenes draw
 * from these (`tour-scenes.js`) and the sound effects land on them (`tour-sound.ts`), so a pop is
 * heard on the frame the window pops. Every scene starts on a whole second, and every cue but the
 * cursor's travel falls on the music's grid (`BEAT`, an eighth or a sixteenth of it), so the presses,
 * pops and rows hit with the beat.
 */
export const CUES = {
  boot: { drop: 0.15, land: 0.7, letters: 1.0, letterGap: 0.125, bar: [2.0, 4.0], bars: 16, jingle: 4.0 },
  rules: { win: 0.5, typeFrom: 0.5, typeRate: 30, dialog: 4.5, cursor: [5.6, 7.2], press: 7.5 },
  ideas: { win: 0.5, row0: 1.5, rowGap: 1.0 },
  orders: { step0: 0.5, stepGap: 1.0, phone: 4.5, buzz: 5.0, cursor: [5.5, 6.3], press: 6.5, approved: 7.0 },
  record: { win: 0.5, row0: 1.0, rowGap: 0.5, cursor: [4.9, 5.6], click: 5.75, tamper: 6.0, cascadeGap: 0.125 },
  check: { win: 0.5, item0: 0.5, itemGap: 2.0, stop: 3.0, cursor: [3.1, 3.8], press: 4.0 },
  end: { win: 0.25, sting: 0.5, perch0: 1.5, perchGap: 0.25, fade: 6 },
} as const;

/** One beat of the music, in seconds: 120 beats a minute. */
export const BEAT = 0.5;

/** The rules typed into the notepad, one character every 1 / `CUES.rules.typeRate` seconds, each a keystroke. */
export const RULES_TEXT = "US stocks only.\nNo more than $2,000 on any one order.\nAsk me before buying anything you haven’t held.";

/** The pixel wipe between scenes: how long the old scene takes to fill, and the new one to clear. */
export const WIPE = 0.32;

/** Frames per second. */
export const FPS = 30;
