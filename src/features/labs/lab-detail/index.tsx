"use client";

import { useState } from "react";
import { PageHeader } from "@/components/ui/page-header";
import { LabBrief } from "@/features/labs/lab-brief";
import { HeaderLead } from "@/features/labs/lab-detail/header-lead";
import { runtimeWord } from "@/features/labs/lab-detail/labels";
import { deployPlacement, diagramAbove, headerMode, WIDE_LAB_PAGE } from "@/features/labs/lab-detail/lab-state";
import { costEstimate } from "@/features/labs/cloud-panel";
import { BackLink } from "@/features/labs/lab-detail/page-parts";
import {
  LabDeployment,
  LabHeaderActions,
  LabNetwork,
  labNetworkView,
  LabNotices,
  LabObjectives,
  LabPanels,
  LabSideRail,
} from "@/features/labs/lab-detail/page-sections";
import { RemoveDialog } from "@/features/labs/lab-detail/remove-dialog";
import { LabDetailSkeleton } from "@/features/labs/lab-detail/skeleton";
import { useEscapeBack } from "@/features/labs/lab-detail/use-escape-back";
import { useLabDetail, type LabDetailProps } from "@/features/labs/lab-detail/use-lab-detail";
import { useT } from "@/lib/i18n";
import { useMediaQuery } from "@/lib/use-media-query";

export type { LabDetailProps } from "@/features/labs/lab-detail/use-lab-detail";

/** A lab's own page: what it is, its state and the one thing to do next, then its deployment,
 *  network, brief and side rail (objective, attack box, where it runs, details). */
export function LabDetail(props: LabDetailProps) {
  const { lab, status, busy, loggedIn, ready = true } = props;
  const t = useT();
  useEscapeBack(props.onBack);
  const page = useLabDetail(props);
  const { s } = page;
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const wide = useMediaQuery(WIDE_LAB_PAGE);

  if (!ready) return <LabDetailSkeleton />;

  const meta = runtimeWord(t, s);
  const mode = headerMode(s, { loggedIn, canLogin: !!props.onLogin, canResume: !!props.onResume });
  const placement = deployPlacement(s, { busy, failed: s.deployFailed });
  // The lab's own hourly estimate, for the aside.
  const cost = s.isCloud ? costEstimate(t, page.preview?.hourlyUsd) : null;
  const deploy = <LabDeployment page={page} props={props} />;
  // On a wide window the diagram takes the page's full width, above the main column and the rail,
  // so a wide lab is drawn whole instead of scrolling sideways beside the rail.
  const above = diagramAbove(wide, labNetworkView(page, props), placement);

  return (
    <div className="animate-rise-in space-y-5">
      <BackLink onBack={props.onBack} />

      {/* Header: what the lab is, its state at a glance, and the one thing to do next. */}
      <PageHeader
        // Opened from the list (whose row is gone now): the focus starts here, not on <body>.
        focusTitle
        title={lab.title}
        lead={<HeaderLead lab={lab} state={s} status={status} hosted={page.hosted.live} />}
        actions={<LabHeaderActions mode={mode} page={page} props={props} onRemove={() => setConfirmingRemove(true)} />}
      />
      <LabNotices page={page} props={props} />

      {above && <LabNetwork page={page} props={props} meta={meta} />}

      <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_19rem] 2xl:grid-cols-[minmax(0,1fr)_22rem]">
        <div className="min-w-0 space-y-5">
          {placement === "top" && deploy}
          <LabPanels page={page} props={props} meta={meta} network={!above} />
          {placement === "bottom" && deploy}
          <LabObjectives page={page} props={props} />
          <LabBrief labId={lab.id} />
        </div>

        <LabSideRail page={page} props={props} meta={meta} cost={cost} />
      </div>
      {confirmingRemove && (
        <RemoveDialog
          title={lab.title}
          canShutdown={s.canShutdown}
          cloud={s.isCloud}
          onCancel={() => setConfirmingRemove(false)}
          onConfirm={() => {
            setConfirmingRemove(false);
            void props.onStop();
          }}
        />
      )}
    </div>
  );
}
