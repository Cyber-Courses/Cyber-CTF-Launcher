"use client";

import { useState } from "react";
import type { MessageKey } from "@/lib/i18n";

// A setting picked from presets, or typed in as a "Custom" value (the attack box image, the
// attack VM's box).

/** The message `key` in the current language, or `english` for a preset without one. */
export const translated = (t: (key: MessageKey) => string, key: MessageKey | undefined, english: string) => (key ? t(key) : english);

/** A setting picked from presets or typed in ("Custom"): its value, read and saved through
 *  `stored`, and the custom field's state (open when the saved value is no preset). */
export function usePresetChoice(stored: { read: () => string; write: (v: string) => void; fallback: string }, presets: readonly string[], onSaved: () => void) {
  const [value, setValue] = useState(stored.read);
  const isPreset = presets.includes(value);
  const [customOpen, setCustomOpen] = useState(!isPreset);
  const [draft, setDraft] = useState(isPreset ? "" : value);
  const choose = (next: string) => {
    const chosen = next.trim() || stored.fallback;
    stored.write(chosen);
    setValue(chosen);
    onSaved();
  };
  return { value, customOpen, setCustomOpen, draft, setDraft, choose };
}
