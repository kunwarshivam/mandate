/** The landing desktop's windows. */
export type AppId = "home" | "record" | "questions" | "guestbook" | "readme" | "owl" | "display" | "tour" | "bin";

/** Each window's short name, on its taskbar button and in the browser's Window menu. */
export const TASK: Record<AppId, string> = {
  home: "Owlhead",
  record: "The record",
  questions: "Questions",
  guestbook: "Guestbook",
  readme: "readme.txt",
  owl: "owl.jpg",
  display: "Display",
  tour: "Tour.mp4",
  bin: "Recycle Bin",
};
