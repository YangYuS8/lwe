<script lang="ts">
  import { onMount } from 'svelte';
  import PageHeader from '$lib/layout/PageHeader.svelte';
  import { copy, setPreferredLanguage } from '$lib/i18n';
  import { Button } from '$lib/ui/button';
  import { Card } from '$lib/ui/card';
  import * as Select from '$lib/ui/select';
  import { loadDiagnostics, loadSettingsPage, updateSettings } from '$lib/ipc';
  import {
    applyThemePreference,
    needsPageLoad,
    pageCache,
    setCurrentPage,
    setSettingsSnapshot
  } from '$lib/stores/ui';
  import type { SettingsPageSnapshot } from '$lib/types';

  export let initialEditing = false;

  const readError = (error: unknown) =>
    error instanceof Error ? error.message : $copy.settings.requestError;

  type SettingsDraft = {
    language: string;
    theme: string;
    launchOnLogin: boolean;
    steamWebApiKey: string;
  };

  const languageOptions = [
    { value: 'en', labelKey: 'en' },
    { value: 'zh-CN', labelKey: 'zh-CN' },
    { value: 'system', labelKey: 'system' }
  ] as const;

  const themeOptions = [
    { value: 'system', labelKey: 'system' },
    { value: 'light', labelKey: 'light' },
    { value: 'dark', labelKey: 'dark' }
  ] as const;

  const createDraft = (snapshot: SettingsPageSnapshot): SettingsDraft => ({
    language: snapshot.language,
    theme: snapshot.theme,
    launchOnLogin: snapshot.launchOnLogin,
    steamWebApiKey: snapshot.steamWebApiKey
  });

  const maskApiKey = (value: string) => {
    if (!value) {
      return $copy.settings.maskedKeyUnavailable;
    }

    if (value.length >= 8) {
      return `${value.slice(0, 4)}...${value.slice(-4)}`;
    }

    return '********';
  };

  const applySnapshot = (snapshot: SettingsPageSnapshot) => {
    setSettingsSnapshot(snapshot);
    setPreferredLanguage(snapshot.language as 'en' | 'zh-CN' | 'system');
    applyThemePreference(snapshot.theme as 'light' | 'dark' | 'system');
    draftSource = snapshot;
    draft = createDraft(snapshot);
  };

  const languageLabel = (value: string) => $copy.settings.languageOptions[value as 'en' | 'zh-CN' | 'system'] ?? value;
  const themeLabel = (value: string) => $copy.settings.themeOptions[value as 'system' | 'light' | 'dark'] ?? value;

  let loading = false;
  let saving = false;
  let pageError: string | null = null;
  let diagnosticsLoading = false;
  let diagnosticsError: string | null = null;
  let diagnosticsText: string | null = null;
  let diagnosticsCopied = false;
  let actionMessage: string | null = null;
  let draftSource: SettingsPageSnapshot | null = null;
  let isEditing = initialEditing;
  let draft: SettingsDraft = {
    language: 'en',
    theme: 'system',
    launchOnLogin: false,
    steamWebApiKey: ''
  };

  $: snapshot = $pageCache.settings.snapshot;
  $: if (snapshot && snapshot !== draftSource) {
    draft = createDraft(snapshot);
    draftSource = snapshot;
  }
  $: hasSnapshot = Boolean(snapshot);
  $: hasChanges =
    snapshot !== null &&
    (draft.language !== snapshot.language ||
      draft.theme !== snapshot.theme ||
      draft.launchOnLogin !== snapshot.launchOnLogin ||
      draft.steamWebApiKey !== snapshot.steamWebApiKey);

  const ensurePage = async () => {
    if (!needsPageLoad('settings')) {
      return;
    }

    loading = true;
    pageError = null;
    actionMessage = null;

    try {
      applySnapshot(await loadSettingsPage());
    } catch (error) {
      pageError = readError(error);
    } finally {
      loading = false;
    }
  };

  const saveSettings = async () => {
    if (!snapshot || !hasChanges) {
      return;
    }

    saving = true;
    pageError = null;
    actionMessage = null;

    try {
      const outcome = await updateSettings({
        language: draft.language !== snapshot.language ? draft.language : null,
        theme: draft.theme !== snapshot.theme ? draft.theme : null,
        launchOnLogin:
          draft.launchOnLogin !== snapshot.launchOnLogin ? draft.launchOnLogin : null,
        steamWebApiKey:
          draft.steamWebApiKey !== snapshot.steamWebApiKey ? draft.steamWebApiKey : null
      });

      if (outcome.currentUpdate) {
        applySnapshot(outcome.currentUpdate);
      }

      actionMessage = outcome.message;
      isEditing = false;
    } catch (error) {
      pageError = readError(error);
    } finally {
      saving = false;
    }
  };

  const startEditing = () => {
    actionMessage = null;
    isEditing = true;
  };

  const cancelEditing = () => {
    if (!snapshot) {
      return;
    }

    draft = createDraft(snapshot);
    draftSource = snapshot;
    actionMessage = null;
    pageError = null;
    isEditing = false;
  };

  const loadDiagnosticsText = async () => {
    diagnosticsLoading = true;
    diagnosticsError = null;
    diagnosticsCopied = false;

    try {
      diagnosticsText = (await loadDiagnostics()).text;
    } catch (error) {
      diagnosticsError = readError(error);
    } finally {
      diagnosticsLoading = false;
    }
  };

  const copyDiagnostics = async () => {
    if (!diagnosticsText) {
      return;
    }

    try {
      await navigator.clipboard.writeText(diagnosticsText);
      diagnosticsCopied = true;
    } catch (error) {
      diagnosticsError = readError(error);
    }
  };

  onMount(() => {
    setCurrentPage('settings');
    void ensurePage();
  });
</script>

<svelte:head>
  <title>{$copy.settings.pageTitle}</title>
</svelte:head>

<section class="grid gap-6">
  <PageHeader
    eyebrow={$copy.settings.pageTitle}
    title={$copy.settings.headerTitle}
    subtitle={$copy.settings.headerSubtitle}
  />

  {#if pageError}
    <p class="lwe-warning-banner" role="alert" aria-live="assertive">{pageError}</p>
  {/if}

  {#if loading && !hasSnapshot}
    <p class="text-sm text-muted-foreground" role="status" aria-live="polite">{$copy.settings.loading}</p>
  {/if}

  {#if snapshot}
    <div class="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(280px,0.9fr)] xl:items-start">
      <Card class="lwe-panel gap-5">
        <div class="grid gap-1.5">
          <p class="lwe-eyebrow">{isEditing ? $copy.settings.editableSettings : $copy.settings.currentSettings}</p>
          <h2 class="lwe-heading-md">{$copy.settings.preferences}</h2>
          <p class="text-sm leading-6 text-muted-foreground">
            {#if isEditing}
              {$copy.settings.editSubtitle}
            {:else}
              {$copy.settings.reviewSubtitle}
            {/if}
          </p>
        </div>

        {#if actionMessage}
          <p class="lwe-info-banner" role="status" aria-live="polite">{actionMessage}</p>
        {/if}

        {#if isEditing}
          <div class="grid gap-4">
            <label class="grid gap-1.5">
              <span class="lwe-eyebrow">{$copy.settings.language}</span>
              <Select.Root type="single" name="settingsLanguage" bind:value={draft.language}>
                <Select.Trigger aria-label={$copy.settings.language} class="min-w-[14rem]">
                  {languageLabel(draft.language)}
                </Select.Trigger>

                <Select.Content>
                  {#each languageOptions as option}
                    <Select.Item value={option.value} label={languageLabel(option.value)}>{languageLabel(option.value)}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </label>

            <label class="grid gap-1.5">
              <span class="lwe-eyebrow">{$copy.settings.theme}</span>
              <Select.Root type="single" name="settingsTheme" bind:value={draft.theme}>
                <Select.Trigger aria-label={$copy.settings.theme} class="min-w-[14rem]">
                  {themeLabel(draft.theme)}
                </Select.Trigger>

                <Select.Content>
                  {#each themeOptions as option}
                    <Select.Item value={option.value} label={themeLabel(option.value)}>{themeLabel(option.value)}</Select.Item>
                  {/each}
                </Select.Content>
              </Select.Root>
            </label>

            {#if snapshot.launchOnLoginAvailable}
              <label class="flex items-start gap-3 rounded-[1rem] border border-border/80 bg-muted/70 p-4">
                <input
                  type="checkbox"
                  bind:checked={draft.launchOnLogin}
                  disabled={saving}
                  aria-label={$copy.settings.launchOnLogin}
                  class="mt-1 h-4 w-4 rounded border-input text-primary focus:ring-ring"
                />
                <span class="grid gap-1.5">
                  <span class="lwe-eyebrow">{$copy.settings.launchOnLogin}</span>
                  <span class="text-sm leading-6 text-foreground/85">
                    {$copy.settings.startOnSession}
                  </span>
                </span>
              </label>
            {:else}
              <div class="grid gap-2 rounded-[1rem] border border-dashed border-border/80 bg-muted/60 p-4">
                <p class="lwe-eyebrow">{$copy.settings.launchOnLogin}</p>
                <p class="text-sm leading-6 text-foreground/85">
                  {$copy.settings.launchOnLoginUnavailable}
                </p>
                <p class="text-sm leading-6 text-muted-foreground">
                  {$copy.settings.launchPreferencePrefix} {draft.launchOnLogin ? $copy.settings.preferEnabled.toLowerCase() : $copy.settings.preferDisabled.toLowerCase()}.
                </p>
              </div>
            {/if}

            <label class="grid gap-1.5">
              <span class="lwe-eyebrow">{$copy.settings.steamWebApiKey}</span>
              <input
                type="password"
                bind:value={draft.steamWebApiKey}
                autocomplete="off"
                spellcheck={false}
                class="h-10 rounded-md border border-input bg-background px-3 text-sm text-foreground"
              />
              <span class="text-xs text-muted-foreground">{$copy.settings.steamWebApiKeyHint}</span>
            </label>

          </div>

          <div class="flex flex-wrap items-center gap-3">
            <Button onclick={saveSettings} disabled={!hasChanges || saving}>
              {saving ? $copy.settings.saving : $copy.settings.saveChanges}
            </Button>
            <Button onclick={cancelEditing} variant="outline" disabled={saving}>{$copy.settings.cancel}</Button>

            {#if hasChanges && !saving}
              <p class="text-sm leading-6 text-muted-foreground" aria-live="polite">{$copy.settings.unsavedChanges}</p>
            {/if}
          </div>
        {:else}
          <div class="grid gap-4 rounded-[1rem] border border-border/80 bg-muted/60 p-4 text-sm leading-6 text-foreground/85">
            <p><span class="font-medium text-foreground">{$copy.settings.language}:</span> {languageLabel(snapshot.language)}</p>
            <p><span class="font-medium text-foreground">{$copy.settings.theme}:</span> {themeLabel(snapshot.theme)}</p>
            <p><span class="font-medium text-foreground">{$copy.settings.steamWebApiKeySaved}</span> {maskApiKey(snapshot.steamWebApiKey)}</p>
            <p>
              <span class="font-medium text-foreground">
                {snapshot.launchOnLoginAvailable ? $copy.settings.launchOnLoginSaved : $copy.settings.savedLaunchPreference}
              </span>
              {#if snapshot.launchOnLoginAvailable}
                {snapshot.launchOnLogin ? $copy.settings.enabled : $copy.settings.disabled}
              {:else}
                {snapshot.launchOnLogin ? $copy.settings.preferEnabled : $copy.settings.preferDisabled}
              {/if}
            </p>
          </div>

          <div class="flex flex-wrap items-center gap-3">
            <Button onclick={startEditing}>{$copy.settings.editSettings}</Button>
          </div>
        {/if}
      </Card>

      <Card class="lwe-panel gap-4">
        <div class="grid gap-1.5">
          <p class="lwe-eyebrow">{$copy.settings.steamIntegration}</p>
          <h2 class="lwe-heading-md">{$copy.settings.currentState}</h2>
        </div>

        <div class="lwe-subpanel gap-2.5">
          <p class="text-sm font-semibold tracking-tight text-foreground">
            {snapshot.steamRequired ? $copy.settings.steamRequired : $copy.settings.steamOptional}
          </p>
          <p class="text-sm leading-6 text-muted-foreground">{snapshot.steamStatusMessage}</p>
        </div>

        <div class="grid gap-1.5 text-sm leading-6 text-muted-foreground">
          <p><span class="font-medium text-foreground">{$copy.settings.savedLanguage}</span> {languageLabel(snapshot.language)}</p>
          <p><span class="font-medium text-foreground">{$copy.settings.savedTheme}</span> {themeLabel(snapshot.theme)}</p>
          <p><span class="font-medium text-foreground">{$copy.settings.steamWebApiKeySaved}</span> {maskApiKey(snapshot.steamWebApiKey)}</p>
          <p>
            <span class="font-medium text-foreground">
              {snapshot.launchOnLoginAvailable ? $copy.settings.launchOnLoginSaved : $copy.settings.savedLaunchPreference}
            </span>
            {#if snapshot.launchOnLoginAvailable}
              {snapshot.launchOnLogin ? $copy.settings.enabled : $copy.settings.disabled}
            {:else}
              {snapshot.launchOnLogin ? $copy.settings.preferEnabled : $copy.settings.preferDisabled}
            {/if}
          </p>
        </div>
      </Card>

    </div>
  {/if}

  <Card class="lwe-panel gap-4">
    <div class="grid gap-1.5">
      <p class="lwe-eyebrow">{$copy.settings.diagnostics}</p>
      <h2 class="lwe-heading-md">{$copy.settings.diagnosticsTitle}</h2>
      <p class="text-sm leading-6 text-muted-foreground">{$copy.settings.diagnosticsDescription}</p>
    </div>

    {#if diagnosticsError}
      <p class="lwe-warning-banner" role="alert" aria-live="assertive">{diagnosticsError}</p>
    {/if}

    <div class="flex flex-wrap gap-3">
      <Button variant="secondary" onclick={loadDiagnosticsText} disabled={diagnosticsLoading}>
        {diagnosticsLoading ? $copy.settings.loadingDiagnostics : $copy.settings.generateDiagnostics}
      </Button>
      <Button variant="outline" onclick={copyDiagnostics} disabled={!diagnosticsText}>
        {diagnosticsCopied ? $copy.settings.diagnosticsCopied : $copy.settings.copyDiagnostics}
      </Button>
    </div>

    {#if diagnosticsText}
      <textarea
        readonly
        class="min-h-72 w-full rounded-md border border-input bg-muted/60 p-3 font-mono text-xs leading-5 text-foreground"
        value={diagnosticsText}
        aria-label={$copy.settings.diagnosticsTitle}
      ></textarea>
    {/if}
  </Card>
</section>
