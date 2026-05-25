<script lang="ts">
  import { Button } from '$lib/ui/button';
  import { Card } from '$lib/ui/card';
  import { Separator } from '$lib/ui/separator';
  import CoverImage from '$lib/components/CoverImage.svelte';
  import {
    copy,
    formatCopy,
    getDesktopRestoreStateLabel,
    getDesktopRuntimeStatusLabel
  } from '$lib/i18n';
  import type { CopyDictionary } from '$lib/i18n';
  import StatusBadge from '$lib/components/StatusBadge.svelte';
  import type { DesktopRestoreState, RuntimeStatus } from '$lib/types';

  export let displayName: string;
  export let monitorId: string;
  export let resolution: string | null = null;
  export let currentItemLabel: string;
  export let currentCoverPath: string | null = null;
  export let clearSupported = false;
  export let clearing = false;
  export let onClear: (() => void) | undefined = undefined;
  export let runtimeStatus: RuntimeStatus | null = null;
  export let restoreState: DesktopRestoreState | null = null;
  export let restoreIssue: string | null = null;
  export let missing = false;
  let detailsExpanded = false;

  type StatusBadgeEntry = {
    label: string;
    variantKey: string;
  };

  const createStatusBadges = (
    copyValue: CopyDictionary,
    runtimeStatus: RuntimeStatus | null,
    restoreState: DesktopRestoreState | null
  ): StatusBadgeEntry[] => {
    const badges: StatusBadgeEntry[] = [];

    if (runtimeStatus) {
      badges.push({
        label: getDesktopRuntimeStatusLabel(copyValue, runtimeStatus),
        variantKey: runtimeStatus
      });
    }

    if (restoreState) {
      badges.push({
        label: getDesktopRestoreStateLabel(copyValue, restoreState),
        variantKey: restoreState
      });
    }

    return badges;
  };

  $: statusBadges = createStatusBadges($copy, runtimeStatus, restoreState);
  $: missingBadgeLabel = getDesktopRestoreStateLabel($copy, 'missing_monitor');
  $: hasStateDetails = statusBadges.length > 0 || Boolean(restoreIssue);
  $: issueBannerClass = restoreIssue ? 'lwe-warning-banner' : 'lwe-info-banner';
  $: desktopMonitorCardCopy = $copy.components.desktopMonitorCard;
</script>

<Card
  class="lwe-panel-compact"
  aria-label={formatCopy(desktopMonitorCardCopy.ariaLabel, { displayName })}
>
  {#if currentCoverPath}
    <CoverImage
      coverPath={currentCoverPath}
      label={formatCopy(desktopMonitorCardCopy.currentItemLabel, { displayName })}
    />
  {/if}

  <div class="grid gap-4 px-1 pb-1">
    <div class="grid gap-3.5">
      <div class="flex flex-wrap items-center gap-2">
        <StatusBadge label={desktopMonitorCardCopy.desktopBadge} variantKey="desktop" />
        {#if missing && restoreState !== 'missing_monitor'}
          <StatusBadge label={missingBadgeLabel} variantKey="missing_monitor" />
        {/if}

        {#each statusBadges as statusBadge}
          <StatusBadge label={statusBadge.label} variantKey={statusBadge.variantKey} />
        {/each}
      </div>

      <div class="grid gap-1.5">
        <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{desktopMonitorCardCopy.monitor}</p>
        <h3 class="lwe-heading-md lwe-wrap-safe">{displayName}</h3>
        <p class="lwe-wrap-safe text-sm text-muted-foreground">
          {monitorId}
          {#if resolution}
            <span class="text-muted-foreground/65"> • </span>{resolution}
          {/if}
        </p>
      </div>
    </div>

    <div class="lwe-subpanel">
      <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{desktopMonitorCardCopy.currentItem}</p>
      <p class="lwe-wrap-safe text-sm leading-6 text-foreground/90">{currentItemLabel}</p>

      {#if clearSupported}
        <div class="mt-3">
          <Button
            variant="outline"
            size="sm"
            class="w-fit focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
            aria-label={formatCopy(desktopMonitorCardCopy.clearWallpaperAriaLabel, { displayName })}
            disabled={clearing}
            onclick={onClear}
          >
            {clearing ? desktopMonitorCardCopy.clearing : desktopMonitorCardCopy.clear}
          </Button>
        </div>
      {/if}
    </div>

    {#if hasStateDetails}
      <Separator class="bg-border/80" />
      <div class="lwe-subpanel gap-3">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{desktopMonitorCardCopy.restoreState}</p>

          <Button
            variant="outline"
            size="sm"
            class="w-fit"
            aria-expanded={detailsExpanded}
            onclick={() => {
              detailsExpanded = !detailsExpanded;
            }}
          >
            {detailsExpanded
              ? desktopMonitorCardCopy.hideStatusDetails
              : desktopMonitorCardCopy.viewStatusDetails}
          </Button>
        </div>

        {#if statusBadges.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each statusBadges as statusBadge}
              <StatusBadge label={statusBadge.label} variantKey={statusBadge.variantKey} />
            {/each}
          </div>
        {/if}

        {#if restoreIssue}
          <p class={issueBannerClass} role="status" aria-live="polite">
            {restoreIssue}
          </p>
        {/if}

        {#if detailsExpanded}
          <div class="grid gap-4 rounded-2xl border border-border/80 bg-card/80 p-4">
            <Separator class="bg-border/80" />

            <div class="grid gap-1.5">
              <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{desktopMonitorCardCopy.monitorStatus}</p>
              <h4 class="lwe-heading-md lwe-wrap-safe">{displayName}</h4>
              <p class="lwe-wrap-safe text-sm text-muted-foreground">{monitorId}</p>
            </div>

            {#if statusBadges.length > 0}
              <div class="flex flex-wrap gap-2">
                {#each statusBadges as statusBadge}
                  <StatusBadge label={statusBadge.label} variantKey={statusBadge.variantKey} />
                {/each}
              </div>
            {/if}

            <p class="lwe-wrap-safe text-sm leading-6 text-foreground/85">
              {restoreIssue ?? desktopMonitorCardCopy.noRestoreIssue}
            </p>

            <p class="text-xs font-medium uppercase tracking-[0.16em] text-muted-foreground">
              {desktopMonitorCardCopy.expandStatusHint}
            </p>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</Card>
