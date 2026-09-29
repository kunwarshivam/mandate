"use client";

import { type FormEvent, useCallback, useEffect, useId, useState } from "react";
import type { PasskeyListItem } from "@supabase/supabase-js";
import { CircleNotch, Fingerprint, Plus } from "@phosphor-icons/react";
import { Section } from "@/components/screens/common";
import { ADD_PASSKEY_COPY, passkeyProblem, webAuthnSupported } from "@/lib/auth-errors";
import { clockShort, dateLabel, zoneLabel } from "@/lib/format";
import { type BrowserClient, createClient } from "@/lib/supabase/client";
import { cn } from "@/lib/utils";
import { FIELD, NOTICE } from "./buttons";

export type PasskeysAuth = Pick<BrowserClient["auth"], "registerPasskey" | "passkey">;

/** Supabase's limit on a passkey's name. */
export const NAME_MAX = 120;

const SMALL_PILL =
  "press inline-flex h-11 shrink-0 items-center justify-center gap-2 rounded-full px-4 text-sm font-semibold outline-none focus-visible:ring-3 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60";
const PRIMARY = `${SMALL_PILL} bg-lapis text-lapis-foreground hover:bg-lapis-strong`;
const OUTLINE = `${SMALL_PILL} border border-foreground/25 bg-card hover:bg-background`;
const QUIET = `${SMALL_PILL} px-3 text-muted-foreground hover:bg-muted hover:text-foreground`;

const LOAD_FAILED = "Your passkeys couldn’t be loaded. Nothing changed; try again.";
const SAVE_FAILED = "That change didn’t save. Nothing changed; try again.";

/** "Sep 29, 2026, 09:14 UTC", read from the timestamp itself, like every time in the app. */
export function passkeyTime(iso: string): string {
  return `${dateLabel(iso)}, ${clockShort(iso)} ${zoneLabel(iso)}`;
}

function nameOf(passkey: PasskeyListItem): string {
  return passkey.friendly_name?.trim() || "Passkey";
}

type Editing = { id: string; mode: "rename"; draft: string } | { id: string; mode: "delete" };

/**
 * The profile's passkeys (DEC-211): each with when it was added and last used, and ways to add,
 * rename and delete one. Deleting the last one leaves Google as the way in, which the copy says.
 */
export function PasskeysSection({ auth }: { auth?: PasskeysAuth }) {
  const [client] = useState<PasskeysAuth>(() => auth ?? createClient().auth);
  const [passkeys, setPasskeys] = useState<PasskeyListItem[] | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const nameId = useId();

  const fetchList = useCallback(async () => {
    const { data, error } = await client.passkey.list();
    return error || !data ? null : data;
  }, [client]);

  const show = useCallback((list: PasskeyListItem[] | null) => {
    setLoadFailed(list === null);
    if (list) setPasskeys(list);
  }, []);

  const load = useCallback(async () => {
    setLoadFailed(false);
    show(await fetchList());
  }, [fetchList, show]);

  useEffect(() => {
    let live = true;
    void fetchList().then((list) => {
      if (live) show(list);
    });
    return () => {
      live = false;
    };
  }, [fetchList, show]);

  async function add() {
    setMessage(null);
    if (!webAuthnSupported()) {
      setMessage(ADD_PASSKEY_COPY.unsupported);
      return;
    }
    setBusy("add");
    const { error } = await client.registerPasskey();
    setBusy(null);
    if (error) {
      setMessage(ADD_PASSKEY_COPY[passkeyProblem(error)]);
      return;
    }
    setMessage("Passkey added.");
    await load();
  }

  async function rename(event: FormEvent<HTMLFormElement>, id: string, draft: string) {
    event.preventDefault();
    const friendlyName = draft.trim();
    if (friendlyName === "") return;
    setMessage(null);
    setBusy(id);
    const { data, error } = await client.passkey.update({ passkeyId: id, friendlyName });
    setBusy(null);
    if (error || !data) {
      setMessage(SAVE_FAILED);
      return;
    }
    setPasskeys((list) => list?.map((p) => (p.id === id ? { ...p, ...data } : p)) ?? null);
    setEditing(null);
    setMessage("Passkey renamed.");
  }

  async function remove(id: string) {
    setMessage(null);
    setBusy(id);
    const { error } = await client.passkey.delete({ passkeyId: id });
    setBusy(null);
    if (error) {
      setMessage(SAVE_FAILED);
      return;
    }
    setPasskeys((list) => list?.filter((p) => p.id !== id) ?? null);
    setEditing(null);
    setMessage("Passkey deleted.");
  }

  const adding = busy === "add";

  return (
    <Section
      title="Passkeys"
      className="max-w-3xl"
      action={
        <button type="button" onClick={add} disabled={busy !== null || passkeys === null} className={PRIMARY}>
          {adding ? <CircleNotch className="size-4 shrink-0 motion-safe:animate-spin" aria-hidden /> : <Plus className="size-4 shrink-0" aria-hidden />}
          Add a passkey
        </button>
      }
    >
      <div data-slot="passkeys" aria-busy={busy !== null || (passkeys === null && !loadFailed)} className="grid gap-(--block-gap)">
        <p className="max-w-measure text-muted-foreground">A passkey signs you in with your fingerprint, face or device PIN. Google stays the other way in.</p>

        <p role="status" aria-live="polite" className={message ? NOTICE : "sr-only"}>
          {message ?? (adding ? "Waiting for your device…" : "")}
        </p>

        {loadFailed ? (
          <div className={cn(NOTICE, "flex flex-wrap items-center justify-between gap-3")}>
            <span>{LOAD_FAILED}</span>
            <button type="button" onClick={() => void load()} className={OUTLINE}>
              Try again
            </button>
          </div>
        ) : passkeys === null ? (
          <p className="flex items-center gap-2 text-muted-foreground">
            <CircleNotch className="size-4 shrink-0 motion-safe:animate-spin" aria-hidden />
            Loading passkeys…
          </p>
        ) : passkeys.length === 0 ? (
          <p className="text-muted-foreground">No passkeys yet. Add one to sign in without Google next time.</p>
        ) : (
          <ul className="grid divide-y divide-border/70">
            {passkeys.map((p) => {
              const rowBusy = busy === p.id;
              const edit = editing?.id === p.id ? editing : null;
              return (
                <li key={p.id} data-slot="passkey" className="grid gap-3 py-3.5">
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                    <Fingerprint className="size-5 shrink-0 text-muted-foreground" aria-hidden />
                    <div className="grid min-w-0 flex-1 gap-0.5">
                      <span className="truncate font-semibold">{nameOf(p)}</span>
                      <span className="text-sm text-muted-foreground tabular">
                        Added {passkeyTime(p.created_at)}. {p.last_used_at ? `Last used ${passkeyTime(p.last_used_at)}.` : "Not used yet."}
                      </span>
                    </div>
                    {edit ? null : (
                      <div className="flex shrink-0 gap-1">
                        <button
                          type="button"
                          onClick={() => setEditing({ id: p.id, mode: "rename", draft: nameOf(p) })}
                          disabled={busy !== null}
                          aria-label={`Rename ${nameOf(p)}`}
                          className={QUIET}
                        >
                          Rename
                        </button>
                        <button
                          type="button"
                          onClick={() => setEditing({ id: p.id, mode: "delete" })}
                          disabled={busy !== null}
                          aria-label={`Delete ${nameOf(p)}`}
                          className={QUIET}
                        >
                          Delete
                        </button>
                      </div>
                    )}
                  </div>

                  {edit?.mode === "rename" ? (
                    <form onSubmit={(e) => void rename(e, p.id, edit.draft)} className="grid gap-2 sm:pl-9">
                      <label htmlFor={`${nameId}-${p.id}`} className="field-label text-muted-foreground">
                        Name
                      </label>
                      <div className="flex flex-wrap gap-2">
                        <input
                          id={`${nameId}-${p.id}`}
                          value={edit.draft}
                          maxLength={NAME_MAX}
                          autoComplete="off"
                          onChange={(e) => setEditing({ ...edit, draft: e.target.value })}
                          className={cn(FIELD, "h-11 min-w-0 flex-1 basis-48")}
                        />
                        <button type="submit" disabled={rowBusy || edit.draft.trim() === ""} className={PRIMARY}>
                          {rowBusy ? <CircleNotch className="size-4 shrink-0 motion-safe:animate-spin" aria-hidden /> : null}
                          Save
                        </button>
                        <button type="button" onClick={() => setEditing(null)} disabled={rowBusy} className={OUTLINE}>
                          Cancel
                        </button>
                      </div>
                    </form>
                  ) : null}

                  {edit?.mode === "delete" ? (
                    <div role="group" aria-label={`Delete ${nameOf(p)}`} className={cn(NOTICE, "grid gap-3 sm:ml-9")}>
                      <p>
                        Delete {nameOf(p)}? The device that holds it will no longer sign you in
                        {passkeys.length === 1 ? "; Google will be the only way in." : "."}
                      </p>
                      <div className="flex flex-wrap gap-2">
                        <button type="button" onClick={() => void remove(p.id)} disabled={rowBusy} className={PRIMARY}>
                          {rowBusy ? <CircleNotch className="size-4 shrink-0 motion-safe:animate-spin" aria-hidden /> : null}
                          Delete passkey
                        </button>
                        <button type="button" onClick={() => setEditing(null)} disabled={rowBusy} className={OUTLINE}>
                          Keep it
                        </button>
                      </div>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Section>
  );
}
