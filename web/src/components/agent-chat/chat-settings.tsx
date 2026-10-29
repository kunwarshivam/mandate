"use client";

import { useEffect, useId, useState } from "react";
import type { ChatModel, ChatSettings, HomeAgent } from "@/api/chat";
import { KEY } from "@/components/kumo/key";
import { usd } from "@/lib/format";
import { useRuntime } from "@/lib/mock-runtime";
import { cn } from "@/lib/utils";

/** A cap the server takes: a plain positive decimal (§4.6.2). */
const CAP = /^(?:0|[1-9]\d*)(?:\.\d+)?$/;
const positive = (text: string) => CAP.test(text) && /[1-9]/.test(text);

const FIELD = "h-11 rounded-xl border border-border bg-card px-3 text-base text-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring";

/** One agent's chat model and daily AI spending cap, saved as `ChatSettingsChanged`. */
function AgentChatSettings({ agent, models }: { agent: HomeAgent; models: ChatModel[] }) {
  const { chat } = useRuntime();
  const id = useId();
  const [saved, setSaved] = useState<ChatSettings | null>(null);
  const [model, setModel] = useState("");
  const [cap, setCap] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void chat.settings(agent.agent_id).then((answer) => {
      if (!live || !answer.ok) return;
      setSaved(answer.value);
      setModel(answer.value.chat_model ?? models[0]?.model_id ?? "");
      setCap(answer.value.daily_cap_usd);
    });
    return () => {
      live = false;
    };
  }, [chat, agent.agent_id, models]);
  const save = () => {
    if (!positive(cap)) return setStatus("The cap must be a positive amount in dollars, such as 5 or 2.50.");
    setStatus("Saving…");
    void chat.saveSettings(agent.agent_id, { chat_model: model, daily_cap_usd: cap }).then((answer) => {
      if (!answer.ok) return setStatus(answer.effect === "unknown" ? "Your deployment may have saved it; reload to check." : "Not saved: your deployment refused it.");
      setSaved(answer.value);
      setStatus("Saved.");
    });
  };
  const changed = saved !== null && (model !== (saved.chat_model ?? models[0]?.model_id ?? "") || cap !== saved.daily_cap_usd);
  return (
    <fieldset className="grid gap-3 rounded-2xl border border-border px-4 py-3" data-slot="agent-chat-settings">
      <legend className="px-1 text-h3">{agent.name}</legend>
      <div className="flex flex-wrap items-end gap-3">
        <label className="grid gap-1" htmlFor={`${id}-model`}>
          <span className="text-label text-muted-foreground">Chat model</span>
          <select id={`${id}-model`} className={FIELD} value={model} onChange={(e) => setModel(e.target.value)} disabled={saved === null}>
            {models.map((m) => (
              <option key={m.model_id} value={m.model_id}>
                {m.label} · {usd(m.input_per_million_usd)} in, {usd(m.output_per_million_usd)} out per million tokens
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1" htmlFor={`${id}-cap`}>
          <span className="text-label text-muted-foreground">Daily AI spending cap, USD</span>
          <input id={`${id}-cap`} inputMode="decimal" className={cn(FIELD, "w-32 tabular")} value={cap} onChange={(e) => setCap(e.target.value.trim())} disabled={saved === null} />
        </label>
        <button type="button" className={KEY} onClick={save} disabled={!changed}>
          Save
        </button>
      </div>
      <p role="status" className="min-h-5 text-sm text-muted-foreground">
        {status}
      </p>
    </fieldset>
  );
}

/**
 * Each agent's chat settings (DEC-2293 items 6 and 14): the model it talks with, from the ones the
 * deployment offers, and the most it may spend on AI a day. At the cap it stops and says so.
 */
export function ChatSettingsSection() {
  const { chat } = useRuntime();
  const [agents, setAgents] = useState<HomeAgent[] | null | string>(null);
  const [models, setModels] = useState<ChatModel[]>([]);
  useEffect(() => {
    let live = true;
    void Promise.all([chat.home(), chat.models()]).then(([home, offered]) => {
      if (!live) return;
      setAgents(home.ok ? home.value : "Your deployment didn't report your agents.");
      if (offered.ok) setModels(offered.value);
    });
    return () => {
      live = false;
    };
  }, [chat]);
  return (
    <section aria-labelledby="chat-settings" className="grid gap-3" data-slot="chat-settings">
      <h2 id="chat-settings" className="text-h2">
        Your agents
      </h2>
      {agents === null ? <p className="text-muted-foreground">Reading…</p> : null}
      {typeof agents === "string" ? <p role="alert">{agents}</p> : null}
      {Array.isArray(agents) && agents.length === 0 ? <p className="text-muted-foreground">No agents yet.</p> : null}
      {Array.isArray(agents) && models.length === 0 && agents.length > 0 ? <p className="text-muted-foreground">Your deployment offers no chat models yet.</p> : null}
      {Array.isArray(agents) && models.length > 0 ? agents.map((a) => <AgentChatSettings key={a.agent_id} agent={a} models={models} />) : null}
    </section>
  );
}
