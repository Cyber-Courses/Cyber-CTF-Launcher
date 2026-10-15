"use client";

import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Segmented } from "@/components/ui/segmented";
import type { LabFilters, RuntimeFilter, StatusFilter } from "@/features/labs/lab-filters";
import { difficultyLabel } from "@/features/labs/use-labs";
import { useT } from "@/lib/i18n";

/** The Labs list's search box and its status, runtime and level filters. */
export function LabsFilterBar({ filters, onChange }: { filters: LabFilters; onChange: (patch: Partial<LabFilters>) => void }) {
  const t = useT();
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="relative min-w-48 flex-1 basis-48 sm:max-w-80">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-faint" />
        <Input
          data-lab-search
          value={filters.query}
          onChange={(e) => {
            onChange({ query: e.target.value });
          }}
          placeholder={t("labs.list.search")}
          aria-label={t("labs.list.search")}
          className="pl-8.5"
        />
      </div>
      <Segmented<StatusFilter>
        label={t("labs.list.status")}
        value={filters.status}
        onChange={(status) => {
          onChange({ status });
        }}
        options={[
          { value: "all", label: t("labs.list.all") },
          { value: "todo", label: t("labs.list.todo") },
          { value: "running", label: t("labs.list.running") },
          { value: "solved", label: t("labs.list.solved") },
        ]}
      />
      <Segmented<RuntimeFilter>
        label={t("labs.list.runtime")}
        value={filters.runtime}
        onChange={(runtime) => {
          onChange({ runtime });
        }}
        options={[
          { value: "all", label: t("labs.list.anyRuntime") },
          { value: "DOCKER", label: t("labs.list.container") },
          { value: "VM", label: t("labs.list.vm") },
          { value: "CLOUD", label: t("labs.list.cloud") },
        ]}
      />
      <Segmented<number>
        label={t("labs.list.level")}
        value={filters.difficulty}
        onChange={(difficulty) => {
          onChange({ difficulty });
        }}
        options={[{ value: 0, label: t("labs.list.anyLevel") }, ...[1, 2, 3].map((d) => ({ value: d, label: difficultyLabel(t, d) }))]}
      />
    </div>
  );
}
