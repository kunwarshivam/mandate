import type { Metadata } from "next";
import { Stub } from "@/components/screens/stub";

export const metadata: Metadata = { title: "Describe an agent" };

export default function NewAgentPage() {
  return (
    <Stub title="Describe an agent">
      <p>
        Not built in this slice. You will describe what the agent should do in your own words, review every field the compiler drafts with where it came from, and confirm
        each section before anything deploys to paper.
      </p>
    </Stub>
  );
}
