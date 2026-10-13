"use client";

import { Skeleton } from "@/components/ui/skeleton";
import { useT } from "@/lib/i18n";

/** The lab page's shape while its first checks (sign-in, the lab's status) are in flight. */
export function LabDetailSkeleton() {
  const t = useT();
  return (
    <div className="space-y-5" aria-busy="true" aria-label={t("labs.detail.loading")}>
      <Skeleton className="h-3 w-16" />
      <div className="flex items-end gap-4">
        <div className="flex-1 space-y-3">
          <Skeleton className="h-8 w-72" />
          <Skeleton className="h-3 w-56" />
          <Skeleton className="h-3.5 w-full max-w-2xl" />
        </div>
        <Skeleton className="h-9 w-28 rounded-full" />
      </div>
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_19rem] 2xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="space-y-5">
          <Skeleton className="h-72 w-full rounded-panel" />
          <Skeleton className="h-28 w-full rounded-panel" />
        </div>
        <div className="space-y-5">
          <Skeleton className="h-40 w-full rounded-panel" />
          <Skeleton className="h-28 w-full rounded-panel" />
        </div>
      </div>
    </div>
  );
}
