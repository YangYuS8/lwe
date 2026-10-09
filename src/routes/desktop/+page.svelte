<script lang="ts">
  import { onMount } from 'svelte';
  import DesktopMonitorCard from '$lib/components/DesktopMonitorCard.svelte';
  import { copy, formatCopy } from '$lib/i18n';
  import { actionOutcomeError } from '$lib/action-outcome';
  import { Button } from '$lib/ui/button';
  import PageHeader from '$lib/layout/PageHeader.svelte';
  import { Card } from '$lib/ui/card';
  import * as Select from '$lib/ui/select';
  import { clearLibraryItemFromMonitor, loadDesktopPage } from '$lib/ipc';
  import { needsPageLoad, pageCache, setCurrentPage, setDesktopSnapshot } from '$lib/stores/ui';
  import { applyDesktopClearInvalidations } from './page-actions';
  import { finishDesktopClear, isDesktopClearInFlight, startDesktopClear } from './clear-state';
  import { resolveDesktopPageState } from './page-state';

  type MonitorFilter = 'all' | 'active' | 'missing';

  const readError = (error: unknown) =>
    error instanceof Error ? error.message : $copy.desktop.requestError;

  let loading = false;
  let pageError: string | null = null;
  let actionError: string | null = null;
  let actionMessage: string | null = null;
  let refreshError: string | null = null;
  let clearingMonitorIds = new Set<string>();
  let monitorFilter: MonitorFilter = 'all';

  $: snapshot = $pageCache.desktop.snapshot;
  $: pageState = snapshot ? resolveDesktopPageState(snapshot, $copy) : null;
  $: visibleMonitors = monitorFilter === 'missing' ? [] : snapshot?.monitors ?? [];
  $: visibleMissingMonitorRestores = monitorFilter === 'active' ? [] : snapshot?.missingMonitorRestores ?? [];
  $: filterEmptyMessage =
    monitorFilter === 'active'
      ? $copy.desktop.filterEmptyActive
      : monitorFilter === 'missing'
        ? $copy.desktop.filterEmptyMissing
        : null;

  const ensurePage = async (force = false) => {
    if (loading || (!force && !needsPageLoad('desktop'))) {
      return;
    }

    loading = true;
    pageError = null;

    try {
      setDesktopSnapshot(await loadDesktopPage());
    } catch (error) {
      pageError = readError(error);
    } finally {
      loading = false;
    }
  };

  const refreshAfterClear = async () => {
    loading = true;
    pageError = null;
    refreshError = null;
    try {
      setDesktopSnapshot(await loadDesktopPage());
    } catch (error) {
      refreshError = formatCopy($copy.actionFeedback.refreshFailed, { error: readError(error) });
    } finally {
      loading = false;
    }
  };

  const clearMonitor = async (monitorId: string) => {
    if (isDesktopClearInFlight(clearingMonitorIds, monitorId)) return;
    clearingMonitorIds = startDesktopClear(clearingMonitorIds, monitorId);
    actionError = null;
    actionMessage = null;
    refreshError = null;

    try {
      const outcome = await clearLibraryItemFromMonitor(monitorId);
      applyDesktopClearInvalidations(outcome.invalidations);
      actionError = actionOutcomeError(outcome, $copy.actionFeedback.failed);
      if (actionError) return;
      actionMessage = outcome.message ?? $copy.actionFeedback.completed;
      await refreshAfterClear();
    } catch (error) {
      actionError = readError(error);
    } finally {
      clearingMonitorIds = finishDesktopClear(clearingMonitorIds, monitorId);
    }
  };

  onMount(() => {
    setCurrentPage('desktop');
    void ensurePage();
  });
</script>

<svelte:head>
  <title>{$copy.desktop.pageTitle}</title>
</svelte:head>

<section class="grid gap-6">
  <PageHeader
    eyebrow={$copy.desktop.pageTitle}
    title={$copy.desktop.headerTitle}
    subtitle={$copy.desktop.headerSubtitle}
  >
    {#snippet actions()}
      <label class="grid justify-items-start gap-1.5">
        <span class="lwe-eyebrow">{$copy.desktop.view}</span>
        <Select.Root type="single" name="monitorFilter" bind:value={monitorFilter}>
          <Select.Trigger aria-label={$copy.desktop.filterAriaLabel} class="min-w-[11rem]">
            {monitorFilter === 'all'
              ? $copy.desktop.filterOptions.all
              : monitorFilter === 'active'
                ? $copy.desktop.filterOptions.active
                : $copy.desktop.filterOptions.missing}
          </Select.Trigger>

          <Select.Content>
            <Select.Item value="all" label={$copy.desktop.filterOptions.all}>{$copy.desktop.filterOptions.all}</Select.Item>
            <Select.Item value="active" label={$copy.desktop.filterOptions.active}>{$copy.desktop.filterOptions.active}</Select.Item>
            <Select.Item value="missing" label={$copy.desktop.filterOptions.missing}>{$copy.desktop.filterOptions.missing}</Select.Item>
          </Select.Content>
        </Select.Root>
      </label>
    {/snippet}
  </PageHeader>

  {#if pageError}
    <div class="grid justify-items-start gap-2">
      <p class="lwe-warning-banner" role="alert" aria-live="assertive">{pageError}</p>
      <Button variant="outline" onclick={() => ensurePage(true)} disabled={loading}>{$copy.actionFeedback.retry}</Button>
    </div>
  {/if}
  {#if actionError}
    <p class="lwe-warning-banner" role="alert" aria-live="assertive">{actionError}</p>
  {/if}
  {#if refreshError}
    <div class="grid justify-items-start gap-2">
      <p class="lwe-warning-banner" role="alert" aria-live="assertive">{refreshError}</p>
      <Button variant="outline" onclick={refreshAfterClear} disabled={loading}>{$copy.actionFeedback.retryRefresh}</Button>
    </div>
  {/if}
  {#if loading && !$pageCache.desktop.snapshot}
    <p class="text-sm text-muted-foreground" role="status" aria-live="polite">{$copy.desktop.loading}</p>
  {:else if snapshot}
    <div class="grid gap-5">
      <Card class="lwe-panel gap-5">
        <div class="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(180px,1fr))]">
          <div class="lwe-subpanel content-start gap-2.5">
            <p class="lwe-eyebrow">{$copy.desktop.monitorsDiscovered}</p>
            <p class="text-[clamp(1.75rem,3vw,2.4rem)] font-semibold tracking-tight text-foreground">
              {snapshot.monitors.length}
            </p>
          </div>

          <div class="lwe-subpanel content-start gap-2.5">
            <p class="lwe-eyebrow">{$copy.desktop.monitorDiscovery}</p>
            <p class="text-sm leading-6 text-muted-foreground">{pageState?.monitorAvailabilityLabel ?? $copy.desktop.no}</p>
          </div>

          <div class="lwe-subpanel content-start gap-2.5">
            <p class="lwe-eyebrow">{$copy.desktop.assignmentPersistence}</p>
            <p class="text-sm leading-6 text-muted-foreground">{pageState?.assignmentAvailabilityLabel ?? $copy.desktop.no}</p>
          </div>

          <div class="lwe-subpanel content-start gap-2.5">
            <p class="lwe-eyebrow">{$copy.desktop.snapshotStale}</p>
            <p class="text-sm leading-6 text-muted-foreground">{snapshot.stale ? $copy.desktop.yes : $copy.desktop.no}</p>
          </div>
        </div>

        {#if pageState?.issueMessages.length}
          <div class="grid gap-2.5" aria-live="polite">
            {#each pageState.issueMessages as issue}
              <p class="lwe-info-banner">{issue}</p>
            {/each}
          </div>
        {/if}

        {#if actionMessage}
          <p class="lwe-info-banner" role="status" aria-live="polite">{actionMessage}</p>
        {/if}

        {#if pageState?.emptyMessage}
          <p class="text-sm leading-6 text-muted-foreground">{pageState.emptyMessage}</p>
        {/if}
      </Card>

      {#if visibleMonitors.length > 0}
        <section class="grid gap-4">
          <div class="grid gap-1.5">
            <p class="lwe-eyebrow">{$copy.desktop.activeOutputs}</p>
            <h2 class="lwe-heading-md">{$copy.desktop.filterOptions.active}</h2>
          </div>

          <div class="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]">
            {#each visibleMonitors as monitor}
              <DesktopMonitorCard
                displayName={monitor.displayName}
                monitorId={monitor.monitorId}
                resolution={monitor.resolution}
                currentItemLabel={monitor.currentWallpaperTitle ?? monitor.currentItemId ?? $copy.desktop.noSavedAssignment}
                currentCoverPath={monitor.currentCoverPath}
                clearSupported={monitor.clearSupported}
                clearing={isDesktopClearInFlight(clearingMonitorIds, monitor.monitorId)}
                onClear={() => clearMonitor(monitor.monitorId)}
                runtimeStatus={monitor.runtimeStatus}
                restoreState={monitor.restoreState ?? null}
                restoreIssue={monitor.restoreIssue ?? null}
              />
            {/each}
          </div>
        </section>
      {:else if monitorFilter === 'active'}
        <p class="text-sm leading-6 text-muted-foreground" role="status" aria-live="polite">{filterEmptyMessage}</p>
      {/if}

      {#if visibleMissingMonitorRestores.length > 0}
        <section class="grid gap-4">
          <h2 class="lwe-heading-md">{$copy.desktop.missingMonitorRestores}</h2>

          <div class="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]">
            {#each visibleMissingMonitorRestores as restore}
              <DesktopMonitorCard
                displayName={restore.monitorId}
                monitorId={restore.monitorId}
                resolution={null}
                currentItemLabel={restore.currentWallpaperTitle ?? restore.currentItemId}
                currentCoverPath={null}
                restoreState={restore.restoreState}
                restoreIssue={restore.restoreIssue ?? null}
                missing={true}
              />
            {/each}
          </div>
        </section>
      {:else if monitorFilter === 'missing'}
        <p class="text-sm leading-6 text-muted-foreground" role="status" aria-live="polite">{filterEmptyMessage}</p>
      {/if}

      <p class="text-sm leading-6 text-muted-foreground">
        {$copy.desktop.runtimeDeferred}
      </p>
    </div>
  {/if}
</section>
