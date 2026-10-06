"use client";

import { useSyncExternalStore } from "react";
import { DropdownMenu } from "@cloudflare/kumo/components/dropdown";
import { Invert } from "pixelarticons/react/Invert.js";
import { Monitor } from "pixelarticons/react/Monitor.js";
import { Moon } from "pixelarticons/react/Moon.js";
import { Sun } from "pixelarticons/react/Sun.js";
import { menuIcon } from "@/components/icon";
import { type ThemePref, isThemePref, writeThemePref } from "@/lib/theme";

const OPTIONS = [
  { id: "light", label: "Light", icon: Sun },
  { id: "dark", label: "Dark", icon: Moon },
  { id: "system", label: "System", icon: Monitor },
] as const;

function watchPref(onChange: () => void): () => void {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme-pref"] });
  return () => observer.disconnect();
}

function currentPref(): ThemePref {
  const value = document.documentElement.dataset.themePref;
  return isThemePref(value) ? value : "system";
}

/**
 * Light, dark, or the system's setting. The trigger's icon never changes, so the server's HTML and
 * the first client render agree whatever the cookie says; the menu marks the current choice.
 */
export function ThemeMenu({ className }: { className?: string }) {
  const pref = useSyncExternalStore(watchPref, currentPref, () => "system" as const);
  return (
    <DropdownMenu>
      <DropdownMenu.Trigger render={<button type="button" />} aria-label="Theme" className={className}>
        <Invert className="size-6" aria-hidden />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Group>
          <DropdownMenu.Label>Theme</DropdownMenu.Label>
          {OPTIONS.map((o) => (
            <DropdownMenu.Item key={o.id} icon={menuIcon(o.icon)} selected={pref === o.id} onClick={() => writeThemePref(o.id)}>
              {o.label}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Group>
      </DropdownMenu.Content>
    </DropdownMenu>
  );
}
