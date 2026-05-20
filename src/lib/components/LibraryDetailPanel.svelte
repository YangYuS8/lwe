<script lang="ts">
  import { Card } from '$lib/ui/card';
  import { Separator } from '$lib/ui/separator';
  import { Button } from '$lib/ui/button';
  import * as Select from '$lib/ui/select';
  import CompatibilityPanel from '$lib/components/CompatibilityPanel.svelte';
  import CoverImage from '$lib/components/CoverImage.svelte';
  import StatusBadge from '$lib/components/StatusBadge.svelte';
  import { copy, getCompatibilityBadgeLabel, getItemTypeLabel, getLibrarySourceLabel } from '$lib/i18n';
  import type { DesktopMonitorSummary, LibraryItemDetail, LibraryPageSnapshot } from '$lib/types';
  import { resolveLibraryAvailabilityIssues } from '../../routes/library/page-state';

  export let detail: LibraryItemDetail | null = null;
  export let snapshot: LibraryPageSnapshot | null = null;
  export let loading = false;
  export let error: string | null = null;
  export let monitors: DesktopMonitorSummary[] = [];
  export let selectedMonitorId = '';
  export let applyDisabled = true;
  export let applying = false;
  export let applyError: string | null = null;
  export let applyMessage: string | null = null;
  export let onApply: (() => void) | undefined = undefined;
  export let onMonitorChange: ((monitorId: string) => void) | undefined = undefined;

  $: availabilitySource = detail ?? snapshot;
  $: libraryDetailCopy = $copy.components.libraryDetail;
  $: issueMessages = availabilitySource ? resolveLibraryAvailabilityIssues(availabilitySource, $copy.library) : [];
  $: assignedMonitorLabels = detail?.assignedMonitorLabels ?? [];
  $: applyUnsupportedMessage = detail && !detail.applySupported ? libraryDetailCopy.applyUnsupported : '';
  $: detailCompatibilityLabel = detail ? getCompatibilityBadgeLabel($copy, detail.compatibility.badge) : '';
  $: detailSourceLabel = detail ? getLibrarySourceLabel($copy, detail.source) : '';
  $: detailItemTypeLabel = detail ? getItemTypeLabel($copy, detail.itemType) : '';
</script>

<Card class="lwe-panel">
  {#if loading}
    <div class="lwe-subpanel gap-3" role="status" aria-live="polite">
      <p class="lwe-eyebrow">{libraryDetailCopy.title}</p>
      <p class="text-sm leading-6 text-muted-foreground">{libraryDetailCopy.loading}</p>
    </div>
  {:else if error}
    <div class="lwe-subpanel gap-3">
      <p class="lwe-eyebrow">{libraryDetailCopy.title}</p>
      <p class="lwe-warning-banner lwe-wrap-safe" role="alert" aria-live="assertive">
        {error}
      </p>
    </div>
  {:else if detail}
    <div class="grid min-w-0 gap-4" data-detail-layout="compact-vertical">
      <section class="grid gap-3.5" data-detail-section="header">
        <div class="grid gap-2">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">
            {libraryDetailCopy.itemTitle}
          </p>
          <h2 class="lwe-heading-lg lwe-wrap-safe">{detail.title}</h2>
        </div>

        <div class="flex flex-wrap gap-2">
          <StatusBadge label={detailCompatibilityLabel} variantKey={detail.compatibility.badge} />
          <StatusBadge label={detailSourceLabel} variantKey={detail.source} />
          <StatusBadge label={detailItemTypeLabel} variantKey={detail.itemType} />
        </div>
      </section>

      <section class="grid gap-3" data-detail-section="quick-status">
        {#if issueMessages.length}
          <div class="grid gap-2.5" aria-live="polite">
            {#each issueMessages as issue}
              <p class="lwe-info-banner lwe-wrap-safe">
                {issue}
              </p>
            {/each}
          </div>
        {/if}

        {#if assignedMonitorLabels.length > 0}
          <div class="lwe-subpanel" aria-live="polite">
            <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">
              {libraryDetailCopy.assignedMonitors}
            </p>
            <p class="lwe-wrap-safe text-sm text-foreground/85">{assignedMonitorLabels.join(' • ')}</p>
          </div>
        {/if}
      </section>

      <section class="lwe-subpanel gap-3.5" data-detail-section="actions" aria-label={libraryDetailCopy.actionsAriaLabel}>
        <div class="grid gap-1.5">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{libraryDetailCopy.actions}</p>
          <h3 class="text-base font-semibold tracking-tight text-foreground">{libraryDetailCopy.apply}</h3>
          <p class="text-sm leading-6 text-muted-foreground">
            {libraryDetailCopy.applyDescription}
          </p>
        </div>

        <div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
          <label class="grid gap-1.5">
            <span class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">
              {libraryDetailCopy.monitor}
            </span>

            <Select.Root
              type="single"
              name="libraryMonitor"
              value={selectedMonitorId}
              onValueChange={(value) => onMonitorChange?.(value)}
              disabled={monitors.length === 0 || Boolean(applyUnsupportedMessage)}
            >
              <Select.Trigger aria-label={libraryDetailCopy.applyTargetMonitor}>
                {selectedMonitorId
                  ? monitors.find((monitor) => monitor.monitorId === selectedMonitorId)?.displayName ?? selectedMonitorId
                  : monitors.length > 0
                    ? libraryDetailCopy.selectMonitor
                    : libraryDetailCopy.noMonitorsAvailable}
              </Select.Trigger>

              <Select.Content>
                {#each monitors as monitor}
                  <Select.Item value={monitor.monitorId} label={monitor.displayName}>
                    {monitor.displayName}
                  </Select.Item>
                {/each}
              </Select.Content>
            </Select.Root>
          </label>

          <Button onclick={onApply} disabled={applyDisabled || applying}>
            {applying ? libraryDetailCopy.applying : libraryDetailCopy.apply}
          </Button>
        </div>

        {#if applyUnsupportedMessage}
          <p class="lwe-info-banner lwe-wrap-safe" role="status" aria-live="polite">{applyUnsupportedMessage}</p>
        {/if}

        {#if applyError}
          <p class="lwe-warning-banner lwe-wrap-safe" role="alert" aria-live="assertive">{applyError}</p>
        {/if}

        {#if applyMessage}
          <p class="lwe-info-banner" role="status" aria-live="polite">{applyMessage}</p>
        {/if}
      </section>

      <section class="grid gap-3" data-detail-section="cover">
        <div class="grid max-w-sm gap-2">
          <div class="grid gap-2">
            <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{libraryDetailCopy.cover}</p>
            <p class="text-sm leading-6 text-muted-foreground">
              {libraryDetailCopy.coverDescription}
            </p>
          </div>
          <CoverImage coverPath={detail.coverPath} label={detail.title} />
        </div>
      </section>

      <section data-detail-section="compatibility">
        <CompatibilityPanel
          compatibility={detail.compatibility}
          badgeLabel={detailCompatibilityLabel}
          badgeVariantKey={detail.compatibility.badge}
        />
      </section>

      <Separator class="bg-border/80" />

      <section class="grid gap-4 sm:grid-cols-2" data-detail-section="metadata">
        <div class="lwe-subpanel">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{libraryDetailCopy.description}</p>
          <p class="lwe-wrap-safe text-sm leading-6 text-foreground/85">
            {detail.description ?? libraryDetailCopy.noDescription}
          </p>
        </div>

        <div class="lwe-subpanel">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{libraryDetailCopy.tags}</p>
          <p class="lwe-wrap-safe text-sm leading-6 text-foreground/85">
            {detail.tags.length > 0 ? detail.tags.join(' • ') : libraryDetailCopy.noTags}
          </p>
        </div>

        <div class="lwe-subpanel sm:col-span-2">
          <p class="text-[0.7rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{libraryDetailCopy.source}</p>
          <p class="lwe-wrap-safe text-sm leading-6 text-foreground/85">{detailSourceLabel}</p>
        </div>
      </section>
    </div>
  {:else}
    <div class="grid gap-4">
      {#if issueMessages.length}
        <div class="grid gap-2.5" aria-live="polite">
          {#each issueMessages as issue}
            <p class="lwe-info-banner lwe-wrap-safe">
              {issue}
            </p>
          {/each}
        </div>
      {/if}

      <div class="lwe-subpanel gap-3 border-dashed" role="status" aria-live="polite">
        <p class="lwe-eyebrow">{libraryDetailCopy.title}</p>
        <p class="text-sm leading-6 text-muted-foreground">{libraryDetailCopy.empty}</p>
      </div>
    </div>
  {/if}
</Card>
