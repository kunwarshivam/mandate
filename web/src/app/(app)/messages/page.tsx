import type { Metadata } from "next";
import { MessagesScreen } from "@/components/messages/messages-screen";

export const metadata: Metadata = { title: "Messages" };

export default function MessagesPage() {
  return <MessagesScreen />;
}
