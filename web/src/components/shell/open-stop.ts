/**
 * How anything opens the frame's one Stop sheet. It sits apart from `stop-control` so that a
 * component the sheet itself renders, such as the mode banner, can open Stop without importing the
 * sheet back.
 */
export const OPEN_STOP_EVENT = "owlhead:open-stop";

export function openStop(): void {
  window.dispatchEvent(new Event(OPEN_STOP_EVENT));
}
