<script lang="ts">
  import { onMount } from 'svelte';
import type { WorkshopOnlineSearchResult } from '$lib/types';
import PageHeader from '$lib/layout/PageHeader.svelte';
import { copy } from '$lib/i18n';
import WorkshopDetailPanel from '$lib/components/WorkshopDetailPanel.svelte';
import { Button } from '$lib/ui/button';
import * as Select from '$lib/ui/select';
  import {
    isLatestWorkshopOnlineSearchResponse,
    isMissingSteamApiKeyError,
    onlineRuntimeStatusKey
  } from './page-state';
  import {
    loadWorkshopItemDetail,
    loadWorkshopPage,
    loadSettingsPage,
    openWorkshopInSteam,
    refreshWorkshopCatalog,
    searchWorkshopOnline,
    updateSettings
  } from '$lib/ipc';
  import {
    setCurrentPage,
    pageCache,
    setSelectedItem,
    setWorkshopDetailIfSelected,
    setWorkshopOnlineCache,
    setWorkshopSnapshot,
    workshopOnlineCache
  } from '$lib/stores/ui';

  const readError = (error: unknown) =>
    error instanceof Error ? error.message : $copy.workshop.requestError;

  const onlineRuntimeLabel = (itemType: 'video' | 'scene' | 'web' | 'application') => {
    return $copy.workshop.runtimeLabels[onlineRuntimeStatusKey(itemType)];
  };

  const onlineRuntimeDescription = (itemType: 'video' | 'scene' | 'web' | 'application') =>
    $copy.workshop.runtimeDescriptions[itemType];

  const localRuntimeDescription = (itemType: 'video' | 'scene' | 'web' | 'application' | 'other') => {
    if (itemType === 'other') {
      return $copy.workshop.runtimeDescriptions.application;
    }

    return $copy.workshop.runtimeDescriptions[itemType];
  };

  let pageError: string | null = null;
  let refreshLoading = false;
  let detailLoading = false;
  let detailError: string | null = null;
  let detailRequestToken = 0;
  let onlineSearchTimer: ReturnType<typeof setTimeout> | null = null;
  let onlineSearchRequestToken = 0;
  let onlineSearchLoading = false;
  let onlineSearchError: string | null = null;
  const initialOnlineSearchCache = $workshopOnlineCache;

  let onlineSearchQuery = initialOnlineSearchCache.query;
  let onlineSearchAgeRatings: ('g' | 'pg_13' | 'r_18')[] = initialOnlineSearchCache.ageRatings;
  let onlineSearchItemTypes: ('video' | 'scene' | 'web' | 'application')[] = initialOnlineSearchCache.itemTypes.length
    ? initialOnlineSearchCache.itemTypes
    : [
    'video',
    'scene',
    'web',
    'application'
  ];
let onlineSearchResult: WorkshopOnlineSearchResult | null = initialOnlineSearchCache.result;
let onlineSearchPage = initialOnlineSearchCache.result?.page ?? 1;
let onlineSearchPageSize = initialOnlineSearchCache.pageSize;
let onlineSearchPageSizeValue = String(initialOnlineSearchCache.pageSize);
let initialOnlineSearchLoading = false;

  const pageSizeOptions = [12, 24, 48, 96] as const;

  let filtersExpanded = false;

  let jumpToPageValue = String(onlineSearchPage);

const pageCount = (result: WorkshopOnlineSearchResult | null) => {
    if (!result?.totalApprox || result.pageSize <= 0) {
      return null;
    }

    return Math.max(1, Math.ceil(result.totalApprox / result.pageSize));
};

  $: workshopSnapshot = $pageCache.workshop.snapshot;
  $: workshopDetail = $pageCache.workshop.detail;
  $: selectedWorkshopId = workshopSnapshot?.selectedItemId ?? null;

  const loadWorkshopSnapshot = async () => {
    pageError = null;

    try {
      setWorkshopSnapshot(await loadWorkshopPage());
    } catch (error) {
      pageError = readError(error);
    }
  };

  const refreshLocalWorkshop = async () => {
    refreshLoading = true;
    pageError = null;

    try {
      const outcome = await refreshWorkshopCatalog();
      if (outcome.currentUpdate) {
        setWorkshopSnapshot(outcome.currentUpdate);
      }
      detailError = null;
    } catch (error) {
      pageError = readError(error);
    } finally {
      refreshLoading = false;
    }
  };

  const selectWorkshopItem = async (workshopId: string) => {
    const requestToken = ++detailRequestToken;
    detailLoading = true;
    detailError = null;
    setSelectedItem('workshop', workshopId);

    try {
      const detail = await loadWorkshopItemDetail(workshopId);
      if (requestToken === detailRequestToken) {
        setWorkshopDetailIfSelected(detail, workshopId);
      }
    } catch (error) {
      if (requestToken === detailRequestToken) {
        detailError = readError(error);
      }
    } finally {
      if (requestToken === detailRequestToken) {
        detailLoading = false;
      }
    }
  };

  const openSelectedWorkshopItemInSteam = async () => {
    if (!selectedWorkshopId) {
      return;
    }

    try {
      await openWorkshopInSteam(selectedWorkshopId);
    } catch (error) {
      detailError = readError(error);
    }
  };

const ensureNonEmptyFilters = () => {
  if (onlineSearchAgeRatings.length === 0) {
    onlineSearchAgeRatings = ['g'];
  }

  if (onlineSearchItemTypes.length === 0) {
    onlineSearchItemTypes = ['video'];
  }
};

  const persistOnlineSearchPreferences = async () => {
    try {
      await updateSettings({
        workshopQuery: onlineSearchQuery,
        workshopAgeRatings: onlineSearchAgeRatings,
        workshopItemTypes: onlineSearchItemTypes
      });
    } catch {
      // Keep interaction responsive even if persistence fails.
    }
  };

const runOnlineSearch = async (options?: { page?: number }) => {
  const requestToken = ++onlineSearchRequestToken;
  if (!onlineSearchResult) {
    initialOnlineSearchLoading = true;
  }
  onlineSearchLoading = true;
  onlineSearchError = null;
    const requestedPage = options?.page ?? 1;

    try {
      const result = await searchWorkshopOnline({
        query: onlineSearchQuery,
        ageRatings: onlineSearchAgeRatings,
        itemTypes: onlineSearchItemTypes,
        page: requestedPage,
        pageSize: onlineSearchPageSize
      });

      if (
        !isLatestWorkshopOnlineSearchResponse({
          requestToken,
          responseToken: onlineSearchRequestToken
        })
      ) {
        return;
      }

      onlineSearchPage = result.page;
      onlineSearchResult = result;
      onlineSearchPageSizeValue = String(onlineSearchPageSize);
      setWorkshopOnlineCache({
        query: onlineSearchQuery,
        ageRatings: onlineSearchAgeRatings,
        itemTypes: onlineSearchItemTypes,
        pageSize: onlineSearchPageSize,
        result: onlineSearchResult
      });
      await persistOnlineSearchPreferences();
    } catch (error) {
      if (
        !isLatestWorkshopOnlineSearchResponse({
          requestToken,
          responseToken: onlineSearchRequestToken
        })
      ) {
        return;
      }

      onlineSearchError = readError(error);
  } finally {
    if (
      isLatestWorkshopOnlineSearchResponse({
        requestToken,
        responseToken: onlineSearchRequestToken
      })
    ) {
      onlineSearchLoading = false;
      initialOnlineSearchLoading = false;
    }
  }
};

  const scheduleOnlineSearch = () => {
  if (onlineSearchTimer) {
    clearTimeout(onlineSearchTimer);
  }

  ensureNonEmptyFilters();
  onlineSearchPage = 1;
  jumpToPageValue = '1';

    onlineSearchTimer = setTimeout(() => {
      void runOnlineSearch({ page: 1 });
    }, 400);
  };

  const triggerOnlineSearchNow = async () => {
  if (onlineSearchTimer) {
    clearTimeout(onlineSearchTimer);
    onlineSearchTimer = null;
  }

  ensureNonEmptyFilters();
  onlineSearchPage = 1;
  jumpToPageValue = '1';
  await runOnlineSearch({ page: 1 });
  };

  const changeOnlineSearchPage = async (direction: 'prev' | 'next') => {
    if (!onlineSearchResult || onlineSearchLoading) {
      return;
    }

    if (direction === 'next' && !onlineSearchResult.hasMore) {
      return;
    }

    if (direction === 'prev' && onlineSearchPage <= 1) {
      return;
    }

    const nextPage = direction === 'next' ? onlineSearchPage + 1 : onlineSearchPage - 1;
    jumpToPageValue = String(nextPage);
    await runOnlineSearch({ page: nextPage });
  };

  const jumpToOnlineSearchPage = async () => {
    if (!onlineSearchResult || onlineSearchLoading) {
      return;
    }

    const requested = Number.parseInt(jumpToPageValue, 10);
    if (!Number.isFinite(requested) || requested < 1) {
      jumpToPageValue = String(onlineSearchPage);
      return;
    }

    const pages = pageCount(onlineSearchResult);
    const target = pages ? Math.min(requested, pages) : requested;
    jumpToPageValue = String(target);
    await runOnlineSearch({ page: target });
  };

  const openOnlineItemInSteam = async (workshopId: string) => {
    try {
      await openWorkshopInSteam(workshopId);
    } catch (error) {
      onlineSearchError = readError(error);
    }
  };

  onMount(() => {
    setCurrentPage('workshop');
    if (!$pageCache.workshop.snapshot || $pageCache.workshop.stale) {
      void loadWorkshopSnapshot();
    }
    const cachedOnlineSearch = $workshopOnlineCache;
    if (cachedOnlineSearch.result) {
      onlineSearchQuery = cachedOnlineSearch.query;
      onlineSearchAgeRatings = cachedOnlineSearch.ageRatings;
      onlineSearchItemTypes = cachedOnlineSearch.itemTypes;
      onlineSearchPageSize = cachedOnlineSearch.pageSize;
      onlineSearchPageSizeValue = String(cachedOnlineSearch.pageSize);
      onlineSearchResult = cachedOnlineSearch.result;
      onlineSearchPage = cachedOnlineSearch.result.page;
      jumpToPageValue = String(onlineSearchPage);
      return;
    }

    void loadSettingsPage()
      .then((settings) => {
        onlineSearchQuery = settings.workshopQuery;
        onlineSearchAgeRatings = settings.workshopAgeRatings.length
          ? settings.workshopAgeRatings
          : ['g', 'pg_13'];
        onlineSearchItemTypes = settings.workshopItemTypes.length
          ? settings.workshopItemTypes
          : ['video', 'scene', 'web', 'application'];
        onlineSearchPageSizeValue = String(onlineSearchPageSize);
        onlineSearchPage = 1;
        jumpToPageValue = '1';
        void runOnlineSearch({ page: 1 });
      })
      .catch(() => {
        onlineSearchQuery = '';
        onlineSearchAgeRatings = ['g', 'pg_13'];
        onlineSearchItemTypes = ['video', 'scene', 'web', 'application'];
        onlineSearchPageSizeValue = String(onlineSearchPageSize);
        onlineSearchPage = 1;
        jumpToPageValue = '1';
        void runOnlineSearch({ page: 1 });
      });
  });
</script>

<svelte:head>
  <title>{$copy.workshop.pageTitle}</title>
</svelte:head>

<section class="grid gap-6">
  <PageHeader
    eyebrow={$copy.workshop.pageTitle}
    title={$copy.workshop.headerTitle}
    subtitle={$copy.workshop.headerSubtitle}
  />

  <section class="grid gap-4 rounded-[1.125rem] border border-border/80 bg-card/90 p-4">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="grid gap-1.5">
        <p class="lwe-eyebrow">{$copy.workshop.localCatalog}</p>
        <h2 class="lwe-heading-md">{$copy.workshop.localCatalogTitle}</h2>
        <p class="text-sm leading-6 text-muted-foreground">{$copy.workshop.localCatalogDescription}</p>
      </div>
      <Button variant="secondary" onclick={refreshLocalWorkshop} disabled={refreshLoading}>
        {refreshLoading ? $copy.workshop.refreshingCatalog : $copy.workshop.refreshCatalog}
      </Button>
    </div>

    <div class="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(320px,0.9fr)] xl:items-start">
      <div class="grid gap-3">
        {#if workshopSnapshot?.items.length}
          <div class="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]">
            {#each workshopSnapshot.items as item}
              <button
                type="button"
                class={`grid gap-3 rounded-[1rem] border p-3 text-left transition hover:bg-accent/15 ${selectedWorkshopId === item.id ? 'border-primary/70 bg-accent/20' : 'border-border/80 bg-card'}`}
                aria-label={$copy.workshop.selectLocalItemLabel.replace('{itemTitle}', item.title)}
                aria-pressed={selectedWorkshopId === item.id}
                on:click={() => {
                  void selectWorkshopItem(item.id);
                }}
              >
                <div class="grid gap-2">
                  <p class="line-clamp-2 text-sm font-semibold text-foreground">{item.title}</p>
                  <div class="flex flex-wrap gap-2 text-[0.72rem] font-semibold uppercase tracking-[0.14em]">
                    <span class="rounded-full border border-border/80 bg-background/70 px-2.5 py-1 text-muted-foreground">
                      {$copy.labels.itemTypes[item.itemType]}
                    </span>
                    <span class="rounded-full border border-border/80 bg-background/70 px-2.5 py-1 text-muted-foreground">
                      {$copy.labels.workshopSyncStatuses[item.syncStatus]}
                    </span>
                    <span class="rounded-full border border-border/80 bg-background/70 px-2.5 py-1 text-muted-foreground">
                      {$copy.labels.compatibilityBadges[item.compatibility.badge]}
                    </span>
                  </div>
                  <p class="text-xs leading-5 text-muted-foreground">
                    {$copy.workshop.syncStatusDescriptions[item.syncStatus]}
                  </p>
                  <p class="text-xs leading-5 text-muted-foreground">
                    {localRuntimeDescription(item.itemType)}
                  </p>
                </div>
              </button>
            {/each}
          </div>
        {:else}
          <p class="lwe-info-banner" role="status" aria-live="polite">{$copy.workshop.empty}</p>
        {/if}
      </div>

      <WorkshopDetailPanel
        detail={workshopDetail}
        loading={detailLoading}
        error={detailError}
        openInSteam={selectedWorkshopId ? openSelectedWorkshopItemInSteam : null}
      />
    </div>
  </section>

  <section class="grid gap-4 rounded-[1.125rem] border border-border/80 bg-card/90 p-4">
    <p class="lwe-eyebrow">{$copy.workshop.onlineSearch}</p>

    <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
      <label class="grid gap-1.5">
        <span class="text-sm font-medium text-foreground">{$copy.workshop.searchLabel}</span>
        <input
          type="text"
          bind:value={onlineSearchQuery}
          placeholder={$copy.workshop.searchPlaceholder}
          class="h-10 rounded-md border border-input bg-background px-3 text-sm text-foreground"
          on:input={scheduleOnlineSearch}
          on:keydown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              void triggerOnlineSearchNow();
            }
          }}
        />
      </label>
      <Button variant="secondary" onclick={triggerOnlineSearchNow} disabled={onlineSearchLoading}>
        {$copy.workshop.searchNow}
      </Button>
    </div>

    <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
      <Button
        variant="outline"
        onclick={() => {
          filtersExpanded = !filtersExpanded;
        }}
      >
        {filtersExpanded ? $copy.workshop.hideFilters : $copy.workshop.showFilters}
      </Button>
    </div>

    {#if filtersExpanded}
      <div class="grid gap-3 md:grid-cols-2">
        <fieldset class="grid gap-2 rounded-[1rem] border border-border/80 bg-muted/60 p-3">
          <legend class="px-1 text-sm font-medium text-foreground">{$copy.workshop.ageRatings}</legend>
          <div class="grid gap-2 sm:grid-cols-3">
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchAgeRatings.includes('g')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
            onlineSearchAgeRatings = Array.from(new Set([...onlineSearchAgeRatings, 'g']));
                  } else {
                    onlineSearchAgeRatings = onlineSearchAgeRatings.filter((rating) => rating !== 'g');
                  }
                  if (onlineSearchAgeRatings.length === 0) {
                    onlineSearchAgeRatings = ['g'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.workshop.ageRatingLabels.g}</span>
            </label>
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchAgeRatings.includes('pg_13')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchAgeRatings = Array.from(new Set([...onlineSearchAgeRatings, 'pg_13']));
                  } else {
                    onlineSearchAgeRatings = onlineSearchAgeRatings.filter((rating) => rating !== 'pg_13');
                  }
                  if (onlineSearchAgeRatings.length === 0) {
                    onlineSearchAgeRatings = ['pg_13'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.workshop.ageRatingLabels.pg_13}</span>
            </label>
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchAgeRatings.includes('r_18')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchAgeRatings = Array.from(new Set([...onlineSearchAgeRatings, 'r_18']));
                  } else {
                    onlineSearchAgeRatings = onlineSearchAgeRatings.filter((rating) => rating !== 'r_18');
                  }
                  if (onlineSearchAgeRatings.length === 0) {
                    onlineSearchAgeRatings = ['r_18'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.workshop.ageRatingLabels.r_18}</span>
            </label>
          </div>
        </fieldset>

        <fieldset class="grid gap-2 rounded-[1rem] border border-border/80 bg-muted/60 p-3">
          <legend class="px-1 text-sm font-medium text-foreground">{$copy.workshop.itemTypes}</legend>
          <div class="grid gap-2 sm:grid-cols-2">
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchItemTypes.includes('video')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchItemTypes = Array.from(new Set([...onlineSearchItemTypes, 'video']));
                  } else {
                    onlineSearchItemTypes = onlineSearchItemTypes.filter((type) => type !== 'video');
                  }
                  if (onlineSearchItemTypes.length === 0) {
                    onlineSearchItemTypes = ['video'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.labels.itemTypes.video}</span>
            </label>
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchItemTypes.includes('scene')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchItemTypes = Array.from(new Set([...onlineSearchItemTypes, 'scene']));
                  } else {
                    onlineSearchItemTypes = onlineSearchItemTypes.filter((type) => type !== 'scene');
                  }
                  if (onlineSearchItemTypes.length === 0) {
                    onlineSearchItemTypes = ['scene'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.labels.itemTypes.scene}</span>
            </label>
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchItemTypes.includes('web')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchItemTypes = Array.from(new Set([...onlineSearchItemTypes, 'web']));
                  } else {
                    onlineSearchItemTypes = onlineSearchItemTypes.filter((type) => type !== 'web');
                  }
                  if (onlineSearchItemTypes.length === 0) {
                    onlineSearchItemTypes = ['web'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.labels.itemTypes.web}</span>
            </label>
            <label class="flex items-center gap-2 text-sm text-foreground/85">
              <input
                type="checkbox"
                checked={onlineSearchItemTypes.includes('application')}
                on:change={(event) => {
                  const target = event.currentTarget as HTMLInputElement;
                  if (target.checked) {
                    onlineSearchItemTypes = Array.from(new Set([...onlineSearchItemTypes, 'application']));
                  } else {
                    onlineSearchItemTypes = onlineSearchItemTypes.filter((type) => type !== 'application');
                  }
                  if (onlineSearchItemTypes.length === 0) {
                    onlineSearchItemTypes = ['application'];
                  }
                  onlineSearchResult = null;
                  scheduleOnlineSearch();
                }}
              />
              <span>{$copy.labels.itemTypes.application}</span>
            </label>
          </div>
        </fieldset>
      </div>
    {/if}

    {#if initialOnlineSearchLoading}
      <div class="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]" aria-busy="true" aria-live="polite">
        {#each Array(6) as _, index (index)}
          <div class="grid gap-2 rounded-[1rem] border border-border/80 bg-card p-3 animate-pulse">
            <div class="aspect-square w-full rounded-[0.9rem] bg-muted"></div>
            <div class="h-4 w-5/6 rounded bg-muted"></div>
            <div class="h-9 w-28 rounded bg-muted"></div>
          </div>
        {/each}
      </div>
    {:else if onlineSearchError}
      <div class="grid gap-2" role="alert" aria-live="assertive">
        <p class="lwe-warning-banner">{onlineSearchError}</p>
        {#if isMissingSteamApiKeyError(onlineSearchError)}
          <a class="lwe-info-banner text-sm font-medium" href="/settings">
            {$copy.workshop.missingApiKeySettingsHint}
          </a>
        {/if}
      </div>
    {:else if onlineSearchResult}
      <div class="grid gap-2">
        <p class="text-sm font-medium text-foreground">{$copy.workshop.onlineResults}</p>
        <p class="text-sm leading-6 text-muted-foreground">{$copy.workshop.onlineResultAcquisitionNote}</p>
        {#if onlineSearchResult.items.length}
          <div class="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(220px,1fr))]">
            {#each onlineSearchResult.items as item}
              <div class="grid gap-2 rounded-[1rem] border border-border/80 bg-card p-3">
                <img
                  src={item.previewUrl ?? undefined}
                  alt={item.title}
                  class="aspect-square w-full rounded-[0.9rem] border border-border/80 bg-muted object-cover"
                  loading="lazy"
                />
                <p class="line-clamp-2 text-sm font-semibold text-foreground">{item.title}</p>
                <div class="grid gap-1.5 rounded-[0.75rem] border border-border/70 bg-muted/50 p-2">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="rounded-full border border-border/80 bg-background px-2 py-0.5 text-xs font-medium text-foreground">
                      {$copy.labels.itemTypes[item.itemType]}
                    </span>
                    <span class="rounded-full border border-border/80 bg-background px-2 py-0.5 text-xs font-medium text-foreground">
                      {onlineRuntimeLabel(item.itemType)}
                    </span>
                  </div>
                  <p class="text-xs leading-5 text-muted-foreground">
                    {onlineRuntimeDescription(item.itemType)}
                  </p>
                </div>
                <Button variant="outline" onclick={() => openOnlineItemInSteam(item.id)}>
                  {$copy.components.workshopDetail.openInSteam}
                </Button>
              </div>
            {/each}
          </div>

          <div class="flex items-center gap-3">
            <Button
              variant="secondary"
              onclick={() => {
                void changeOnlineSearchPage('prev');
              }}
              disabled={onlineSearchPage <= 1 || onlineSearchLoading}
            >
              {$copy.workshop.previousPage}
            </Button>
            <p class="text-xs text-muted-foreground">{$copy.workshop.pageLabel} {onlineSearchPage}</p>
            {#if pageCount(onlineSearchResult) !== null}
              <p class="text-xs text-muted-foreground">/ {pageCount(onlineSearchResult)}</p>
            {/if}
            <label class="flex items-center gap-2 text-xs text-muted-foreground">
              <span>{$copy.workshop.pageSize}</span>
              <Select.Root
                type="single"
                bind:value={onlineSearchPageSizeValue}
                onValueChange={(value) => {
                  onlineSearchPageSize = Number(value);
                  onlineSearchPageSizeValue = value;
                  onlineSearchPage = 1;
                  jumpToPageValue = '1';
                  void runOnlineSearch({ page: 1 });
                }}
              >
                <Select.Trigger aria-label={$copy.workshop.pageSize} class="min-w-[5rem]">
                  {onlineSearchPageSize}
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
                    void jumpToOnlineSearchPage();
                  }
                }}
              />
              <Button
                variant="outline"
                onclick={() => {
                  void jumpToOnlineSearchPage();
                }}
                disabled={onlineSearchLoading}
              >
                {$copy.workshop.goToPage}
              </Button>
            </label>
            <Button
              variant="secondary"
              onclick={() => {
                void changeOnlineSearchPage('next');
              }}
              disabled={!onlineSearchResult.hasMore || onlineSearchLoading}
            >
              {$copy.workshop.nextPage}
            </Button>
          </div>
        {:else}
          <p class="text-sm text-muted-foreground">{$copy.workshop.noOnlineResults}</p>
        {/if}
      </div>
    {/if}
  </section>

  {#if pageError}
    <p class="lwe-warning-banner" role="alert" aria-live="assertive">{pageError}</p>
  {/if}
</section>
