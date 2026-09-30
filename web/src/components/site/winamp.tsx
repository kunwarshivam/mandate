"use client";

import { useEffect, useRef } from "react";
import Webamp from "webamp";
import { PLAYLIST } from "./music";

export type AmpState = "off" | "open" | "minimized";

/** Above the desktop's windows, under the taskbar and its Start menu. */
const LAYER = 50;

/**
 * Winamp 2, as Webamp (webamp.org, MIT) rebuilds it for the browser: the main window, equalizer
 * and playlist, each dragged by its own title bar and snapping to the others. Webamp draws itself at
 * the top of the document, centred on `anchor`; minimizing hides it to the taskbar, and closing it
 * turns it off until its icon opens it again.
 */
export default function Winamp({ anchor, state, onState }: { anchor: HTMLElement; state: AmpState; onState: (s: AmpState) => void }) {
  const amp = useRef<Webamp | null>(null);
  const closed = useRef(false);
  const report = useRef(onState);

  useEffect(() => {
    report.current = onState;
  }, [onState]);

  useEffect(() => {
    if (!Webamp.browserIsSupported()) {
      report.current("off");
      return;
    }
    const webamp = new Webamp({
      initialTracks: PLAYLIST.map(({ url, duration, metaData }) => ({ url, duration, metaData })),
      windowLayout: {
        main: { position: { top: 0, left: 0 } },
        equalizer: { position: { top: 116, left: 0 } },
        playlist: { position: { top: 232, left: 0 }, size: { extraHeight: 1, extraWidth: 0 } },
      },
      zIndex: LAYER,
    });
    amp.current = webamp;
    const offClose = webamp.onClose(() => {
      closed.current = true;
      report.current("off");
    });
    const offMinimize = webamp.onMinimize(() => report.current("minimized"));
    void webamp.renderWhenReady(anchor);
    return () => {
      offClose();
      offMinimize();
      webamp.dispose();
      amp.current = null;
    };
  }, [anchor]);

  useEffect(() => {
    const el = document.getElementById("webamp");
    if (el) el.style.display = state === "minimized" ? "none" : "";
    if (state === "open" && closed.current) {
      closed.current = false;
      amp.current?.reopen();
    }
  }, [state]);

  return null;
}
