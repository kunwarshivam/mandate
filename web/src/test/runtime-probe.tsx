import { useEffect } from "react";
import { type Runtime, useRuntime } from "@/lib/mock-runtime";

const seen: { runtime: Runtime | null } = { runtime: null };

/** Rendered inside the providers, it lets a test read and drive the runtime the screens use. */
export function RuntimeProbe() {
  const runtime = useRuntime();
  useEffect(() => {
    seen.runtime = runtime;
  }, [runtime]);
  return null;
}

/** The runtime as of the last render, after effects. */
export function probed(): Runtime {
  if (!seen.runtime) throw new Error("render <RuntimeProbe /> inside the providers first");
  return seen.runtime;
}
