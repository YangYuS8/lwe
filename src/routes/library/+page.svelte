<script lang="ts">
  import { onMount } from 'svelte';
  import type { InvalidatedPage } from '$lib/types';
  import ItemCard from '$lib/components/ItemCard.svelte';
  import LibraryDetailPanel from '$lib/components/LibraryDetailPanel.svelte';
  import PageHeader from '$lib/layout/PageHeader.svelte';
  import { copy, formatCopy } from '$lib/i18n';
  import {
    applyLibraryItemToMonitor,
    loadDesktopPage,
    loadLibraryItemDetail,
    loadLibraryPage,
    loadSettingsPage,
    refreshWorkshopCatalog,
    updateSettings
  } from '$lib/ipc';
  import { Button } from '$lib/ui/button';
  import * as Select from '$lib/ui/select';
  import {
    applyInvalidations,
    isSelectedItem,
    needsPageLoad,
    pageCache,
    setCurrentPage,
    setDesktopSnapshot,
    setLibraryDetailIfSelected,
    setLibrarySnapshot,
    setSelectedItem
  } from '$lib/stores/ui';
  import { resolveLibraryApplyRefreshState, resolveLibraryPageState } from './page-state';

  const readError = (error: unknown) =>
    error instanceof Error ? error.message : $copy.library.requestError;

  let loading = false;
  let detailLoading = false;
  let pageError: string | null = null;
  let detailError: string | null = null;
  let applyError: string | null = null;
  let applyMessage: string | null = null;
  let applyLoading = false;
  let applyMonitorId = '';
  let detailRequestToken = 0;
  let filterPanelExpanded = false;
  let pageSizeValue = '24';
  let currentPage = 1;
  let jumpToPageValue = '1';
  let filterAgeRatings: ('g' | 'pg_13' | 'r_18')[] = ['g', 'pg_13'];
  let filterItemTypes: ('video' | 'scene' | 'web' | 'application')[] = [
    'video',
    'scene',
    'web',
    'application'
  ];
  const pageSizeOptions = [12, 24, 48, 96] as const;

  $: pageSize = Number(pageSizeValue);
  $: filteredItems = (snapshot?.items ?? []).filter((item) => {
    const normalizedItemType =
      item.itemType === 'other' ? 'application' : (item.itemType as 'video' | 'scene' | 'web' | 'application');
    const itemTypeMatches = filterItemTypes.includes(normalizedItemType);
    const itemAgeRating = (item.ageRating ?? 'g') as 'g' | 'pg_13' | 'r_18';
    const ageMatches = filterAgeRatings.includes(itemAgeRating);
    return itemTypeMatches && ageMatches;
  });
  $: totalPages = Math.max(1, Math.ceil(filteredItems.length / pageSize));
  $: pagedItems = filteredItems.slice((currentPage - 1) * pageSize, currentPage * pageSize);

  $: snapshot = $pageCache.library.snapshot;
  $: pageState = snapshot ? resolveLibraryPageState(snapshot, $copy.library) : null;
  $: desktopSnapshot = $pageCache.desktop.snapshot;
  $: availableMonitors = desktopSnapshot?.monitors ?? [];
  $: selectedDetail = $pageCache.library.detail;
  $: {
    if (currentPage > totalPages) {
      currentPage = totalPages;
    }
    if (currentPage < 1) {
      currentPage = 1;
    }
  }
  $: {
    if (!availableMonitors.length) {
      applyMonitorId = '';
    } else if (!availableMonitors.some((monitor) => monitor.monitorId === applyMonitorId)) {
      applyMonitorId = availableMonitors[0]?.monitorId ?? '';
    }
  }

  const ensurePage = async () => {
    if (!needsPageLoad('library')) {
      return;
    }

    loading = true;
    pageError = null;

    try {
      setLibrarySnapshot(await loadLibraryPage());
    } catch (error) {
      pageError = readError(error);
    } finally {
      loading = false;
    }
  };

  const persistLibraryFilters = async () => {
    try {
      await updateSettings({
        workshopAgeRatings: filterAgeRatings,
        workshopItemTypes: filterItemTypes
      });
    } catch {
      // Ignore persistence failure to keep interactions responsive.
    }
  };

  const ensureDesktopSnapshot = async () => {
    if (!needsPageLoad('desktop')) {
      return;
    }

    try {
      setDesktopSnapshot(await loadDesktopPage());
    } catch {
      // Keep the primary action visible even if monitor discovery fails.
    }
  };

  const loadSelectedDetail = async (itemId: string) => {
    const requestToken = ++detailRequestToken;
    detailLoading = true;
    detailError = null;

    try {
      const detail = await loadLibraryItemDetail(itemId);
      if (requestToken !== detailRequestToken) {
        return;
      }

      setLibraryDetailIfSelected(detail, itemId);
    } catch (error) {
      if (requestToken !== detailRequestToken || !isSelectedItem('library', itemId)) {
        return;
      }

      setLibraryDetailIfSelected(null, itemId);
      detailError = readError(error);
    } finally {
      if (requestToken === detailRequestToken) {
        detailLoading = false;
      }
    }
  };

  const refreshInvalidatedPages = async (invalidations: InvalidatedPage[]) => {
    let refreshedSelectedItemId = snapshot?.selectedItemId ?? null;
    let librarySnapshotRefreshSucceeded = true;
    const initialRefreshState = resolveLibraryApplyRefreshState({
      invalidations,
      selectedItemId: refreshedSelectedItemId
    });

    if (initialRefreshState.refreshLibrarySnapshot) {
      try {
        const refreshedSnapshot = await loadLibraryPage();
        setLibrarySnapshot(refreshedSnapshot);
        refreshedSelectedItemId = refreshedSnapshot.selectedItemId;
      } catch (error) {
        librarySnapshotRefreshSucceeded = false;

        const message = readError(error);
        if (snapshot) {
          applyError = message;
        } else {
          pageError = message;
        }
      }
    }

    const refreshState = resolveLibraryApplyRefreshState({
      invalidations,
      selectedItemId: refreshedSelectedItemId,
      librarySnapshotRefreshSucceeded
    });

    if (refreshState.refreshLibraryDetailId) {
      await loadSelectedDetail(refreshState.refreshLibraryDetailId);
    }

    if (refreshState.refreshDesktopSnapshot) {
      await ensureDesktopSnapshot();
    }
  };

  const selectItem = async (itemId: string) => {
    setSelectedItem('library', itemId);
    applyError = null;
    applyMessage = null;

    await loadSelectedDetail(itemId);
  };

  const applySelectedItem = async () => {
    if (!selectedDetail || !applyMonitorId) {
      return;
    }

    applyLoading = true;
    applyError = null;
    applyMessage = null;

    try {
      const outcome = await applyLibraryItemToMonitor(applyMonitorId, selectedDetail.id);
      applyMessage = outcome.message;
      applyInvalidations(outcome.invalidations);
      await refreshInvalidatedPages(outcome.invalidations);
    } catch (error) {
      applyError = readError(error);
    } finally {
      applyLoading = false;
    }
  };

  const refreshLibraryFromWorkshop = async () => {
    loading = true;
    pageError = null;

    try {
      await refreshWorkshopCatalog();
      setLibrarySnapshot(await loadLibraryPage());
    } catch (error) {
      pageError = readError(error);
    } finally {
      loading = false;
    }
  };

  const changePage = (direction: 'prev' | 'next') => {
    if (direction === 'prev') {
      currentPage = Math.max(1, currentPage - 1);
      jumpToPageValue = String(currentPage);
      return;
    }

    currentPage = Math.min(totalPages, currentPage + 1);
    jumpToPageValue = String(currentPage);
  };

  const jumpToPage = () => {
    const parsed = Number.parseInt(jumpToPageValue, 10);
    if (!Number.isFinite(parsed) || parsed < 1) {
      jumpToPageValue = String(currentPage);
      return;
    }

    currentPage = Math.min(parsed, totalPages);
    jumpToPageValue = String(currentPage);
  };

  onMount(() => {
    setCurrentPage('library');
    void ensurePage();
    void ensureDesktopSnapshot();
    void loadSettingsPage()
      .then((settings) => {
        filterAgeRatings = settings.workshopAgeRatings.length
          ? settings.workshopAgeRatings
          : ['g', 'pg_13'];
        filterItemTypes = settings.workshopItemTypes.length
          ? settings.workshopItemTypes
          : ['video', 'scene', 'web', 'application'];
      })
      .catch(() => {
        filterAgeRatings = ['g', 'pg_13'];
        filterItemTypes = ['video', 'scene', 'web', 'application'];
      });
  });
</script>

<svelte:head>
  <title>{$copy.library.pageTitle}</title>
</svelte:head>

<section class="grid gap-6">
  <PageHeader
    eyebrow={$copy.library.pageTitle}
    title={$copy.library.headerTitle}
    subtitle={$copy.library.headerSubtitle}
  >
    {#snippet actions()}
      <Button variant="secondary" onclick={refreshLibraryFromWorkshop} disabled={loading}>
        {$copy.workshop.refreshCatalog}
      </Button>
    {/snippet}
  </PageHeader>

  {#if pageError && !snapshot}
    <p class="lwe-warning-banner" role="alert" aria-live="assertive">{pageError}</p>
  {:else if loading && !snapshot}
    <p class="text-sm text-muted-foreground" role="status" aria-live="polite">{$copy.library.loading}</p>
  {:else if snapshot}
    <div class="grid gap-5 xl:grid-cols-[minmax(0,1.4fr)_minmax(300px,0.9fr)] xl:items-start">
      <section class="grid gap-4">
        {#if pageError}
          <p class="lwe-warning-banner" role="alert" aria-live="assertive">{pageError}</p>
        {/if}

        {#if pageState?.issueMessages.length}
          <div class="grid gap-2.5" aria-live="polite">
            {#each pageState.issueMessages as issue}
              <p class="lwe-info-banner">{issue}</p>
            {/each}
          </div>
        {/if}

        <div class="grid gap-3 rounded-[1rem] border border-border/80 bg-muted/60 p-3">
          <div class="flex items-center justify-between gap-3">
            <Button
              variant="outline"
              onclick={() => {
                filterPanelExpanded = !filterPanelExpanded;
              }}
            >
              {filterPanelExpanded ? $copy.workshop.hideFilters : $copy.workshop.showFilters}
            </Button>

            <div class="flex items-center gap-3">
              <p class="text-xs text-muted-foreground">{$copy.workshop.pageLabel} {currentPage} / {totalPages}</p>

              <label class="flex items-center gap-2 text-xs text-muted-foreground">
                <span>{$copy.workshop.pageSize}</span>
                <Select.Root
                  type="single"
                  bind:value={pageSizeValue}
                  onValueChange={(value) => {
                    pageSizeValue = value;
                    currentPage = 1;
                    jumpToPageValue = '1';
                  }}
                >
                  <Select.Trigger aria-label={$copy.workshop.pageSize} class="min-w-[5rem]">
                    {pageSize}
                  </Select.Trigger>
                  <Select.Content>
                    {#each pageSizeOptions as size}
                      <Select.Item value={String(size)} label={String(size)}>{size}</Select.Item>
                    {/each}
                  </Select.Content>
                </Select.Root>
              </label>

              <label class="flex items-center gap-2 text-xs text-muted-foreground">
                <span>{$copy.workshop.jumpToPage}</span>
                <input
                  type="number"
                  min="1"
                  bind:value={jumpToPageValue}
                  class="h-8 w-16 rounded-md border border-input bg-background px-2 text-xs text-foreground"
                  on:keydown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault();
                      jumpToPage();
                    }
                  }}
                />
                <Button variant="outline" onclick={jumpToPage}>{$copy.workshop.goToPage}</Button>
              </label>

              <Button variant="secondary" onclick={() => changePage('prev')} disabled={currentPage <= 1}>
                {$copy.workshop.previousPage}
              </Button>
              <Button
                variant="secondary"
                onclick={() => changePage('next')}
                disabled={currentPage >= totalPages}
              >
                {$copy.workshop.nextPage}
              </Button>
            </div>
          </div>

          {#if filterPanelExpanded}
            <div class="grid gap-3 md:grid-cols-2">
              <fieldset class="grid gap-2 rounded-[1rem] border border-border/80 bg-card p-3">
                <legend class="px-1 text-sm font-medium text-foreground">{$copy.workshop.ageRatings}</legend>
                <div class="grid gap-2 sm:grid-cols-3">
                  <label class="flex items-center gap-2 text-sm text-foreground/85">
                    <input
                      type="checkbox"
                      checked={filterAgeRatings.includes('g')}
                      on:change={(event) => {
                        const checked = (event.currentTarget as HTMLInputElement).checked;
                        filterAgeRatings = checked
                          ? Array.from(new Set([...filterAgeRatings, 'g']))
                          : filterAgeRatings.filter((entry) => entry !== 'g');
                        if (filterAgeRatings.length === 0) filterAgeRatings = ['g'];
                        currentPage = 1;
                        jumpToPageValue = '1';
                        void persistLibraryFilters();
                      }}
                    />
                    <span>{$copy.workshop.ageRatingLabels.g}</span>
                  </label>
                  <label class="flex items-center gap-2 text-sm text-foreground/85">
                    <input
                      type="checkbox"
                      checked={filterAgeRatings.includes('pg_13')}
                      on:change={(event) => {
                        const checked = (event.currentTarget as HTMLInputElement).checked;
                        filterAgeRatings = checked
                          ? Array.from(new Set([...filterAgeRatings, 'pg_13']))
                          : filterAgeRatings.filter((entry) => entry !== 'pg_13');
                        if (filterAgeRatings.length === 0) filterAgeRatings = ['pg_13'];
                        currentPage = 1;
                        jumpToPageValue = '1';
                        void persistLibraryFilters();
                      }}
                    />
                    <span>{$copy.workshop.ageRatingLabels.pg_13}</span>
                  </label>
                  <label class="flex items-center gap-2 text-sm text-foreground/85">
                    <input
                      type="checkbox"
                      checked={filterAgeRatings.includes('r_18')}
                      on:change={(event) => {
                        const checked = (event.currentTarget as HTMLInputElement).checked;
                        filterAgeRatings = checked
                          ? Array.from(new Set([...filterAgeRatings, 'r_18']))
                          : filterAgeRatings.filter((entry) => entry !== 'r_18');
                        if (filterAgeRatings.length === 0) filterAgeRatings = ['r_18'];
                        currentPage = 1;
                        jumpToPageValue = '1';
                        void persistLibraryFilters();
                      }}
                    />
                    <span>{$copy.workshop.ageRatingLabels.r_18}</span>
                  </label>
                </div>
              </fieldset>

              <fieldset class="grid gap-2 rounded-[1rem] border border-border/80 bg-card p-3">
                <legend class="px-1 text-sm font-medium text-foreground">{$copy.workshop.itemTypes}</legend>
                <div class="grid gap-2 sm:grid-cols-2">
                  {#each ['video', 'scene', 'web', 'application'] as type}
                    <label class="flex items-center gap-2 text-sm text-foreground/85">
                      <input
                        type="checkbox"
                        checked={filterItemTypes.includes(type as 'video' | 'scene' | 'web' | 'application')}
                        on:change={(event) => {
                          const checked = (event.currentTarget as HTMLInputElement).checked;
                          const typed = type as 'video' | 'scene' | 'web' | 'application';
                          filterItemTypes = checked
                            ? Array.from(new Set([...filterItemTypes, typed]))
                            : filterItemTypes.filter((entry) => entry !== typed);
                          if (filterItemTypes.length === 0) filterItemTypes = [typed];
                          currentPage = 1;
                          jumpToPageValue = '1';
                          void persistLibraryFilters();
                        }}
                      />
                      <span>{$copy.labels.itemTypes[type as 'video' | 'scene' | 'web' | 'application']}</span>
                    </label>
                  {/each}
                </div>
              </fieldset>
            </div>
          {/if}
        </div>

        {#if pagedItems.length}
          <div class="grid gap-4 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]">
            {#each pagedItems as item}
              <ItemCard
                title={item.title}
                coverPath={item.coverPath}
                selected={snapshot.selectedItemId === item.id}
                assignedMonitorLabels={item.assignedMonitorLabels ?? []}
                selectLabel={formatCopy($copy.library.selectItemLabel, { itemTitle: item.title })}
                onSelect={() => selectItem(item.id)}
              />
            {/each}
          </div>
        {:else}
          <p class="text-sm leading-6 text-muted-foreground">
            {pageState?.emptyMessage ?? $copy.library.empty}
          </p>
        {/if}
      </section>

      <LibraryDetailPanel
        detail={$pageCache.library.detail}
        snapshot={snapshot}
        loading={detailLoading}
        error={detailError}
        monitors={availableMonitors}
        selectedMonitorId={applyMonitorId}
        applyDisabled={!selectedDetail || !selectedDetail.applySupported || !applyMonitorId}
        applying={applyLoading}
        {applyError}
        applyMessage={applyMessage}
        onApply={applySelectedItem}
        onMonitorChange={(monitorId) => {
          applyMonitorId = monitorId;
        }}
      />
    </div>
  {/if}
</section>
