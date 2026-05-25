<script lang="ts">
import { Card } from '$lib/ui/card';
  import CoverImage from '$lib/components/CoverImage.svelte';
import { copy } from '$lib/i18n';
import type { CompatibilitySummaryModel, ItemType } from '$lib/types';

  export let title: string;
export let coverPath: string | null = null;
export let selected = false;
export let assignedMonitorLabels: string[] = [];
export let itemType: ItemType | null = null;
export let compatibility: CompatibilitySummaryModel | null = null;
export let applySupported = false;
export let selectLabel: string | null = null;
export let onSelect: (() => void) | undefined = undefined;

$: runtimeCopy = applySupported
  ? $copy.components.itemCard.runtimeRunnable
  : $copy.components.itemCard.runtimeUnavailable;
</script>

<Card
  class={`relative lwe-panel-compact group transition duration-150 hover:-translate-y-0.5 hover:border-border/90 hover:bg-accent/15 hover:shadow-[0_24px_56px_rgba(15,23,42,0.12)] ${selected ? 'border-primary/70 ring-1 ring-primary/20' : ''}`}
>
  {#if onSelect && selectLabel}
    <button
      type="button"
      class="absolute inset-0 z-10 rounded-[1.125rem] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      aria-label={selectLabel}
      aria-pressed={selected}
      onclick={onSelect}
    ></button>
  {/if}

  <div class={`grid gap-4 ${onSelect ? 'pointer-events-none relative z-0' : ''}`}>
    <CoverImage {coverPath} label={title} square={true} />

    <div class="grid min-w-0 gap-2 px-1 pb-1">
      <h3 class="line-clamp-2 text-base font-semibold leading-6 text-foreground">{title}</h3>

      <div class="flex flex-wrap gap-2 text-[0.72rem] font-semibold uppercase tracking-[0.14em]">
        {#if itemType}
          <span class="rounded-full border border-border/80 bg-background/70 px-2.5 py-1 text-muted-foreground">
            {$copy.labels.itemTypes[itemType]}
          </span>
        {/if}
        {#if compatibility}
          <span class="rounded-full border border-border/80 bg-background/70 px-2.5 py-1 text-muted-foreground">
            {$copy.labels.compatibilityBadges[compatibility.badge]}
          </span>
        {/if}
        <span
          class={`rounded-full border px-2.5 py-1 ${applySupported ? 'border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300' : 'border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-300'}`}
        >
          {runtimeCopy}
        </span>
      </div>

      {#if !applySupported}
        <p class="text-xs leading-5 text-muted-foreground">{$copy.components.itemCard.runtimeUnavailableDetail}</p>
      {/if}

      {#if assignedMonitorLabels.length > 0}
        <div class="lwe-subpanel gap-1.5 px-3.5 py-3">
          <p class="text-[0.68rem] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
            {$copy.components.itemCard.assignedTo}
          </p>
          <p class="text-sm text-foreground/85">{assignedMonitorLabels.join(' • ')}</p>
        </div>
      {/if}
    </div>
  </div>
</Card>
