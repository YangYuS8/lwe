<script lang="ts">
  import { resolveCoverSrc, shouldRenderCoverImage } from '$lib/components/cover-image';
  import { copy, formatCopy } from '$lib/i18n';

  export let coverPath: string | null = null;
  export let label = 'cover';
  export let square = false;

  let loadFailed = false;
  let previousCoverPath: string | null = null;

  $: if (coverPath !== previousCoverPath) {
    previousCoverPath = coverPath;
    loadFailed = false;
  }

  $: showCoverImage = shouldRenderCoverImage(coverPath, loadFailed);
  $: resolvedCoverSrc = resolveCoverSrc(coverPath);
  $: coverImageCopy = $copy.components.coverImage;
  $: placeholderAriaLabel = formatCopy(coverImageCopy.placeholderAriaLabel, { label });
</script>

{#if showCoverImage}
  <img
    class={`block w-full rounded-[1.35rem] border border-border/80 bg-muted object-cover shadow-[inset_0_1px_0_rgba(255,255,255,0.45),0_16px_40px_rgba(15,23,42,0.08)] ${square ? 'aspect-square' : 'aspect-[16/9]'}`}
    src={resolvedCoverSrc}
    alt={label}
    width={square ? 320 : 1600}
    height={square ? 320 : 900}
    loading="lazy"
    decoding="async"
    on:error={() => (loadFailed = true)}
  />
{:else}
  <div
    class={`grid w-full place-items-center rounded-[1.35rem] border border-dashed border-border/80 bg-gradient-to-br from-muted/80 via-muted/55 to-muted px-4 text-center shadow-[inset_0_1px_0_rgba(255,255,255,0.5)] ${square ? 'aspect-square' : 'aspect-[16/9]'}`}
    aria-label={placeholderAriaLabel}
  >
    <div class="grid gap-1.5">
      <span class="text-[0.68rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">{coverImageCopy.noCover}</span>
      <span class="text-sm leading-6 text-muted-foreground">{coverImageCopy.placeholderDescription}</span>
    </div>
  </div>
{/if}
