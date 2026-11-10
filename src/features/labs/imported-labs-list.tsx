"use client";

import { Panel, PanelHeader } from "@/components/ui/panel";
import { LabRow } from "@/features/labs/lab-row";
import { catalogueRowProps, type RowContext } from "@/features/labs/labs-list";
import type { Lab } from "@/features/labs/use-labs";
import { useT } from "@/lib/i18n";

/** The labs imported from a folder installed here, under their own heading, apart from the
 *  catalogue (its filters and counts are the catalogue's). Each row opens the lab's page, and
 *  stops or resumes it, as a catalogue row does; it never asks to sign in (an imported lab runs
 *  without Cyber CTF). */
export function ImportedLabs({ labs, ctx }: { labs: Lab[] | null; ctx: RowContext }) {
  const t = useT();
  if (!labs?.length) return null;
  const local: RowContext = { ...ctx, onLogin: undefined };
  return (
    <Panel>
      <PanelHeader title={t("labs.list.imported")} meta={String(labs.length)} />
      <div>
        {labs.map((lab) => (
          <LabRow key={lab.id} {...catalogueRowProps(lab, local)} />
        ))}
      </div>
    </Panel>
  );
}
