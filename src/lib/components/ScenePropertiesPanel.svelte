<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { Button } from '$lib/ui/button';
  import { copy, formatCopy } from '$lib/i18n';
  import { chooseSceneImage, loadSceneProperties, updateSceneProperties } from '$lib/ipc';
  import type { ActionOutcome, ScenePropertiesSnapshot, SceneProperty, ScenePropertyValue } from '$lib/types';

  interface Props {
    itemId: string;
    monitorId: string;
    disabled?: boolean;
    onOutcome?: (outcome: ActionOutcome<null>) => void;
  }

  let { itemId, monitorId, disabled = false, onOutcome }: Props = $props();
  const panelId = $props.id();
  let expanded = $state(false);
  let snapshot = $state<ScenePropertiesSnapshot | null>(null);
  let draft = $state<Record<string, boolean | string | null | [number, number, number]>>({});
  let groupExpanded = $state(new Map<string, boolean>());
  let loading = $state(false);
  let saving = $state(false);
  let importing = $state(false);
  let error = $state<string | null>(null);
  let refreshError = $state<string | null>(null);
  let message = $state<string | null>(null);
  let activeKey = '';
  let requestToken = 0;
  let destroyed = false;

  const selectionKey = $derived(JSON.stringify([itemId, monitorId]));
  const panelCopy = $derived($copy.components.sceneProperties);
  const controlsDisabled = $derived(disabled || loading || saving || importing);
  const hasEditableProperties = $derived(snapshot?.properties.some((property) => property.kind !== 'text_display') ?? false);
  const values = $derived(readDraft());
  const groupedNames = $derived(new Set(snapshot?.groups.flatMap((group) => group.members) ?? []));
  const ungrouped = $derived(snapshot?.properties.filter((property) => !groupedNames.has(property.name)) ?? []);
  const propertyControls = $derived(new Map(snapshot?.properties.map((property, index) =>
    [property.name, { property, inputId: `${panelId}-${index}` }] as const) ?? []));

  $effect(() => {
    const key = selectionKey;
    untrack(() => {
      if (activeKey === key) return;
      activeKey = key;
      requestToken += 1;
      expanded = false;
      snapshot = null;
      draft = {};
      groupExpanded = new Map();
      loading = false;
      saving = false;
      importing = false;
      error = null;
      refreshError = null;
      message = null;
    });
  });

  onDestroy(() => { destroyed = true; requestToken += 1; });

  const isCurrent = (token: number, key: string) =>
    !destroyed && requestToken === token && selectionKey === key;
  const finiteOrNull = (value: unknown) => value === null || (typeof value === 'number' && Number.isFinite(value));
  const isImageToken = (value: unknown): value is string => typeof value === 'string' && /^sha256:[a-f0-9]{64}$/.test(value);
  const utf8 = new TextEncoder();
  const isTextInput = (value: unknown): value is string => typeof value === 'string' &&
    value.length <= 256 && utf8.encode(value).length <= 256 && value.split('\n').length <= 8 &&
    !/[\p{Cc}\p{Cs}]/u.test(value.replaceAll('\n', ''));
  const isColor = (value: unknown): value is [number, number, number] =>
    Array.isArray(value) && value.length === 3 && value.every((channel) =>
      typeof channel === 'number' && Number.isFinite(channel) && channel >= 0 && channel <= 1);

  function validSnapshot(value: unknown, expectedItem: string, expectedMonitor: string): value is ScenePropertiesSnapshot {
    if (!value || typeof value !== 'object') return false;
    const candidate = value as ScenePropertiesSnapshot;
    if (candidate.itemId !== expectedItem || candidate.monitorId !== expectedMonitor ||
        !Array.isArray(candidate.properties) || !Array.isArray(candidate.groups) ||
        candidate.properties.length + candidate.groups.length > 256) return false;
    const names = new Set<string>();
    if (!candidate.properties.every((property) => {
      if (!property || typeof property !== 'object' || typeof property.name !== 'string' || !property.name ||
          typeof property.label !== 'string' || names.has(property.name)) return false;
      names.add(property.name);
      if (![property.min, property.max, property.step, property.order].every(finiteOrNull) ||
          (property.min !== null && property.max !== null && property.min > property.max) ||
          (property.step !== null && property.step <= 0)) return false;
      if (property.kind === 'bool') return typeof property.value === 'boolean';
      if (property.kind === 'color') return isColor(property.value);
      if (property.kind === 'texture') return property.value === null || isImageToken(property.value);
      if (property.kind === 'text_display') return property.value === null;
      if (property.kind === 'text_input') return isTextInput(property.value);
      if (property.kind === 'combo') {
        if (typeof property.value !== 'string' || !Array.isArray(property.options) ||
            property.options.length === 0 || property.options.length > 256) return false;
        const options = new Set<string>();
        return property.options.every((option) => {
          if (!option || typeof option !== 'object' || typeof option.label !== 'string' ||
              typeof option.value !== 'string' || !option.value || options.has(option.value)) return false;
          options.add(option.value);
          return true;
        }) && options.has(property.value);
      }
      return property.kind === 'slider' && typeof property.value === 'number' && Number.isFinite(property.value) &&
        (property.min === null || property.value >= property.min) &&
        (property.max === null || property.value <= property.max);
    })) return false;
    const groupNames = new Set<string>();
    const members = new Set<string>();
    return candidate.groups.every((group) => {
      if (!group || typeof group !== 'object' || typeof group.name !== 'string' || !group.name ||
          names.has(group.name) || groupNames.has(group.name) || typeof group.label !== 'string' ||
          !finiteOrNull(group.order) || !Array.isArray(group.members)) return false;
      groupNames.add(group.name);
      return group.members.every((name) => {
        if (typeof name !== 'string' || !names.has(name) || members.has(name)) return false;
        members.add(name);
        return true;
      });
    });
  }

  function toggleGroup(name: string) {
    if (controlsDisabled) return;
    const next = new Map(groupExpanded);
    next.set(name, !(groupExpanded.get(name) ?? true));
    groupExpanded = next;
  }

  function readDraft(): Record<string, ScenePropertyValue> | null {
    if (!snapshot) return null;
    const entries: [string, ScenePropertyValue][] = [];
    for (const property of snapshot.properties) {
      if (property.kind === 'text_display') continue;
      const value = draft[property.name];
      if (property.kind === 'bool') {
        if (typeof value !== 'boolean') return null;
        entries.push([property.name, value]);
      } else if (property.kind === 'color') {
        if (!isColor(value)) return null;
        entries.push([property.name, value]);
      } else if (property.kind === 'combo') {
        if (typeof value !== 'string' || !property.options.some((option) => option.value === value)) return null;
        entries.push([property.name, value]);
      } else if (property.kind === 'texture') {
        if (value === null) continue;
        if (!isImageToken(value)) return null;
        entries.push([property.name, value]);
      } else if (property.kind === 'text_input') {
        if (!isTextInput(value)) return null;
        entries.push([property.name, value]);
      } else {
        if (typeof value !== 'string' || !value.trim()) return null;
        const number = Number(value);
        if (!Number.isFinite(number) || (property.min !== null && number < property.min) ||
            (property.max !== null && number > property.max)) return null;
        entries.push([property.name, number]);
      }
    }
    return Object.fromEntries(entries);
  }

  function edit(name: string, value: boolean | string | null | [number, number, number]) {
    draft = { ...draft, [name]: value };
    error = null;
    message = null;
  }

  const hexColor = (color: [number, number, number]) => '#' + color.map((channel) =>
    Math.round(channel * 255).toString(16).padStart(2, '0')).join('');
  const rgbColor = (hex: string): [number, number, number] => [
    parseInt(hex.slice(1, 3), 16) / 255,
    parseInt(hex.slice(3, 5), 16) / 255,
    parseInt(hex.slice(5, 7), 16) / 255
  ];
  const errorText = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

  async function chooseImage(name: string) {
    if (controlsDisabled) return;
    const token = ++requestToken;
    const key = selectionKey;
    importing = true;
    error = null;
    try {
      const image = await chooseSceneImage(itemId, name);
      if (!isCurrent(token, key) || image === null) return;
      if (!image || !isImageToken(image.token) || !Number.isInteger(image.width) ||
          !Number.isInteger(image.height) || image.width < 1 || image.height < 1 ||
          image.width > 8192 || image.height > 8192) throw new Error(panelCopy.invalidImage);
      edit(name, image.token);
    } catch (reason) { if (isCurrent(token, key)) error = errorText(reason); }
    finally { if (isCurrent(token, key)) importing = false; }
  }

  async function fetchSnapshot(token: number, key: string, currentItem: string, currentMonitor: string) {
    const result = await loadSceneProperties(currentMonitor, currentItem);
    if (!isCurrent(token, key)) return;
    if (!validSnapshot(result, currentItem, currentMonitor)) throw new Error(panelCopy.invalidSnapshot);
    groupExpanded = new Map(result.groups.map((group) => [group.name, groupExpanded.get(group.name) ?? true]));
    snapshot = result;
    draft = Object.fromEntries(result.properties.filter((property) => property.kind !== 'text_display').map((property) => [property.name,
      property.kind === 'slider' ? String(property.value) : property.kind === 'color' ? [...property.value] : property.value
    ]));
  }

  async function reload() {
    if (controlsDisabled) return;
    const token = ++requestToken;
    const key = selectionKey;
    const currentItem = itemId;
    const currentMonitor = monitorId;
    loading = true;
    error = null;
    refreshError = null;
    try { await fetchSnapshot(token, key, currentItem, currentMonitor); }
    catch (reason) {
      if (isCurrent(token, key)) {
        if (snapshot) refreshError = errorText(reason);
        else error = errorText(reason);
      }
    } finally { if (isCurrent(token, key)) loading = false; }
  }

  function toggle() {
    if (disabled || saving || importing) return;
    expanded = !expanded;
    if (expanded && !snapshot && !loading) void reload();
  }

  async function save(reset: boolean) {
    if (controlsDisabled || (snapshot && !hasEditableProperties) || (!reset && !values)) return;
    const submitted = reset ? {} : values!;
    const token = ++requestToken;
    const key = selectionKey;
    const currentItem = itemId;
    const currentMonitor = monitorId;
    saving = true;
    error = null;
    refreshError = null;
    message = null;
    try {
      const outcome = await updateSceneProperties(currentMonitor, currentItem, submitted);
      if (!outcome || typeof outcome.ok !== 'boolean') throw new Error(panelCopy.invalidOutcome);
      // The mutation can complete after a selection change; its invalidations still matter.
      if (!destroyed) onOutcome?.(outcome);
      if (!isCurrent(token, key)) return;
      if (outcome.ok !== true) { error = outcome.message ?? $copy.actionFeedback.failed; return; }
      message = outcome.message ?? panelCopy.saved;
      loading = true;
      try { await fetchSnapshot(token, key, currentItem, currentMonitor); }
      catch (reason) { if (isCurrent(token, key)) refreshError = errorText(reason); }
    } catch (reason) { if (isCurrent(token, key)) error = errorText(reason); }
    finally { if (isCurrent(token, key)) { saving = false; loading = false; } }
  }
</script>

{#snippet propertyControl(property: SceneProperty, inputId: string)}
  {#if property.kind === 'text_display'}
    <p class="lwe-wrap-safe whitespace-pre-wrap text-sm">{property.label}</p>
  {:else if property.kind === 'text_input'}
    {@const valid = isTextInput(draft[property.name])}
    <div class="grid min-w-0 gap-1.5 text-sm">
      <label class="lwe-wrap-safe" for={inputId}>{property.label}</label>
      <textarea id={inputId} value={draft[property.name] as string} disabled={controlsDisabled} rows={2}
        aria-invalid={!valid} aria-describedby={valid ? undefined : `${inputId}-limit`}
        oninput={(event) => edit(property.name, event.currentTarget.value)}
        class="w-full min-w-0 resize-y rounded-md border border-input bg-background px-3 py-2 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"></textarea>
      {#if !valid}<p id={`${inputId}-limit`} class="text-muted-foreground">{panelCopy.textInputLimit}</p>{/if}
    </div>
  {:else if property.kind === 'bool'}
    <label class="flex min-w-0 items-start gap-2 text-sm" for={inputId}>
      <input id={inputId} type="checkbox" checked={draft[property.name] as boolean} disabled={controlsDisabled}
        onchange={(event) => edit(property.name, event.currentTarget.checked)}
        class="mt-1 h-4 w-4 shrink-0 rounded border-input focus:ring-ring" />
      <span class="lwe-wrap-safe">{property.label}</span>
    </label>
  {:else if property.kind === 'slider'}
    <label class="grid min-w-0 gap-1.5 text-sm" for={inputId}>
      <span class="lwe-wrap-safe">{property.label}</span>
      <input id={inputId} type="number" value={draft[property.name] as string} disabled={controlsDisabled}
        min={property.min ?? undefined} max={property.max ?? undefined} step={property.step ?? 'any'}
        oninput={(event) => edit(property.name, event.currentTarget.value)}
        class="h-9 w-full min-w-0 rounded-md border border-input bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" />
    </label>
  {:else if property.kind === 'combo'}
    <label class="grid min-w-0 gap-1.5 text-sm" for={inputId}>
      <span class="lwe-wrap-safe">{property.label}</span>
      <select id={inputId} value={draft[property.name] as string} disabled={controlsDisabled}
        onchange={(event) => edit(property.name, event.currentTarget.value)}
        class="h-9 w-full min-w-0 rounded-md border border-input bg-background px-3 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50">
        {#each property.options as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </label>
  {:else if property.kind === 'texture'}
    <div class="grid min-w-0 gap-1.5 text-sm" role="group" aria-labelledby={inputId}
      data-scene-image-property={property.name} data-image-selected={draft[property.name] !== null}>
      <span id={inputId} class="lwe-wrap-safe">{property.label}</span>
      <p class="text-muted-foreground">{draft[property.name] === null ? panelCopy.authorImage : panelCopy.selectedImage}</p>
      <div class="flex flex-wrap gap-2">
        <Button variant="outline" disabled={controlsDisabled} onclick={() => chooseImage(property.name)}>{panelCopy.chooseImage}</Button>
        <Button variant="outline" disabled={controlsDisabled || draft[property.name] === null} onclick={() => edit(property.name, null)}>{panelCopy.clearImage}</Button>
      </div>
      <p class="text-xs leading-5 text-muted-foreground">{panelCopy.imageFormats}</p>
    </div>
  {:else}
    <label class="grid min-w-0 gap-1.5 text-sm" for={inputId}>
      <span class="lwe-wrap-safe">{property.label}</span>
      <input id={inputId} type="color" value={hexColor(draft[property.name] as [number, number, number])}
        disabled={controlsDisabled} oninput={(event) => edit(property.name, rgbColor(event.currentTarget.value))}
        class="h-9 w-20 rounded-md border border-input bg-background p-1 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50" />
    </label>
  {/if}
{/snippet}

<section class="lwe-subpanel gap-3" data-detail-section="scene-properties" aria-busy={loading || saving || importing}>
  <Button variant="outline" class="h-auto min-h-9 justify-between whitespace-normal text-left" onclick={toggle}
    aria-label={panelCopy.title} aria-expanded={expanded} aria-controls={panelId} disabled={disabled || saving || importing}>
    {panelCopy.title}<span aria-hidden="true">{expanded ? '−' : '+'}</span>
  </Button>
  {#if expanded}
    <div id={panelId} class="grid min-w-0 gap-3">
      <p class="text-sm leading-6 text-muted-foreground">{panelCopy.description}</p>
      {#if loading}<p class="text-sm text-muted-foreground" role="status">{panelCopy.loading}</p>{/if}
      {#if importing}<p class="text-sm text-muted-foreground" role="status">{panelCopy.importingImage}</p>{/if}
      {#if snapshot}
        {#if !hasEditableProperties}
          <p class="text-sm text-muted-foreground">{panelCopy.empty}</p>
        {/if}
        {#if snapshot.properties.length > 0 || snapshot.groups.length > 0}
          <fieldset class="grid min-w-0 gap-3" disabled={controlsDisabled}>
            <legend class="sr-only">{panelCopy.title}</legend>
            {#each ungrouped as property (property.name)}
              {@render propertyControl(property, propertyControls.get(property.name)!.inputId)}
            {/each}
            {#each snapshot.groups as group, index (group.name)}
              {@const headingId = `${panelId}-group-heading-${index}`}
              {@const contentId = `${panelId}-group-content-${index}`}
              {@const open = groupExpanded.get(group.name) ?? true}
              <section class="grid min-w-0 gap-3 border-t pt-3" aria-labelledby={headingId} data-scene-property-group={group.name}>
                <h4 class="min-w-0">
                  <Button id={headingId} variant="ghost" class="h-auto min-h-9 w-full justify-between whitespace-normal px-0 text-left"
                    onclick={() => toggleGroup(group.name)} aria-expanded={open} aria-controls={contentId} disabled={controlsDisabled}>
                    <span class="lwe-wrap-safe">{group.label.trim() ? group.label : formatCopy(panelCopy.unnamedGroup, { number: index + 1 })}</span>
                    <span class="shrink-0" aria-hidden="true">{open ? '−' : '+'}</span>
                  </Button>
                </h4>
                <div id={contentId} hidden={!open}>
                  <div class="grid min-w-0 gap-3">
                    {#each group.members as name (name)}
                      {@const control = propertyControls.get(name)!}
                      {@render propertyControl(control.property, control.inputId)}
                    {:else}
                      <p class="text-sm text-muted-foreground">{panelCopy.emptyGroup}</p>
                    {/each}
                  </div>
                </div>
              </section>
            {/each}
          </fieldset>
        {/if}
        {#if hasEditableProperties}
          {#if !values}<p class="lwe-warning-banner lwe-wrap-safe" role="alert">{panelCopy.invalidValues}</p>{/if}
          <div class="flex flex-wrap gap-2">
            <Button onclick={() => save(false)} disabled={controlsDisabled || !values}>{saving ? panelCopy.saving : panelCopy.save}</Button>
            <Button variant="outline" onclick={() => save(true)} disabled={controlsDisabled}>{panelCopy.reset}</Button>
          </div>
        {/if}
      {/if}
      {#if error}
        <p class="lwe-warning-banner lwe-wrap-safe" role="alert">{error}</p>
        {#if !snapshot}
          <div class="flex flex-wrap gap-2">
            <Button variant="outline" onclick={reload} disabled={controlsDisabled}>{$copy.actionFeedback.retry}</Button>
            <Button variant="outline" onclick={() => save(true)} disabled={controlsDisabled}>{panelCopy.reset}</Button>
          </div>
        {/if}
      {/if}
      {#if message}<p class="lwe-info-banner lwe-wrap-safe" role="status">{message}</p>{/if}
      {#if refreshError}
        <p class="lwe-warning-banner lwe-wrap-safe" role="alert">{panelCopy.refreshFailed} {refreshError}</p>
        <Button variant="outline" onclick={reload} disabled={controlsDisabled}>{$copy.actionFeedback.retryRefresh}</Button>
      {/if}
    </div>
  {/if}
</section>
