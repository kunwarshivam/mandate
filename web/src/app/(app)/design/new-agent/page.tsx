import type { Metadata } from "next";
import { NewAgentPrototype } from "@/components/new-agent/new-agent-prototype";

export const metadata: Metadata = { title: "New agent prototype" };

export default function NewAgentPrototypePage() {
  return <NewAgentPrototype />;
}
