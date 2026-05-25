import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import SettingsPage from './+page.svelte';
import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import { pageCache, setSettingsSnapshot } from '$lib/stores/ui';

const resetCache = () => {
  pageCache.set({
    library: { snapshot: null, detail: null, stale: false },
    workshop: { snapshot: null, detail: null, stale: false },
    desktop: { snapshot: null, detail: null, stale: false },
    settings: { snapshot: null, detail: null, stale: false }
  });
};

describe('settings page render', () => {
  afterEach(() => {
    resetPreferredLanguage();
    resetCache();
  });

  it('renders the settled settings view with an explicit edit action', () => {
    setSettingsSnapshot({
      language: 'en',
      theme: 'system',
      launchOnLogin: true,
      launchOnLoginAvailable: true,
      steamWebApiKey: 'abcd1234wxyz',
      workshopQuery: 'forest',
      workshopAgeRatings: ['g', 'pg_13'],
      workshopItemTypes: ['video', 'scene', 'web', 'application'],
      steamRequired: true,
      steamStatusMessage: 'Steam is required to launch Wallpaper Engine content.',
      stale: false
    });

    const { body } = render(SettingsPage);

    expect(body).toContain('Current settings');
    expect(body).toContain('Edit settings');
    expect(body).toContain('Steam integration');
    expect(body).toContain('Copyable diagnostics');
    expect(body).toContain('Generate diagnostics');
    expect(body).toContain('Steam is required to launch Wallpaper Engine content.');
    expect(body).toContain('English');
    expect(body).toContain('Follow system theme');
    expect(body).toContain('Steam Web API key:');
    expect(body).toContain('abcd...wxyz');
    expect(body).toContain('Launch on login:');
    expect(body).toContain('enabled');
    expect(body).not.toContain('Save changes');
    expect(body).not.toContain('Rust backend');
    expect(body).not.toContain('backend-owned settings file');
    expect(body).not.toContain('Snapshot stale');
  });

  it('renders zh-CN as a selectable language while editing', () => {
    setSettingsSnapshot({
      language: 'zh-CN',
      theme: 'dark',
      launchOnLogin: false,
      launchOnLoginAvailable: true,
      steamWebApiKey: 'test-key',
      workshopQuery: 'city',
      workshopAgeRatings: ['g', 'pg_13'],
      workshopItemTypes: ['video', 'scene', 'web', 'application'],
      steamRequired: false,
      steamStatusMessage: 'Steam is optional for the current setup.',
      stale: false
    });

    const { body } = render(SettingsPage, { props: { initialEditing: true } });

    expect(body).toContain('Language');
    expect(body).toContain('Steam Web API key');
    expect(body).toContain('Simplified Chinese');
    expect(body).toContain('Save changes');
    expect(body).toContain('Cancel');
  });

  it('renders settings copy in Simplified Chinese when zh-CN is active', () => {
    setPreferredLanguage('zh-CN');

    setSettingsSnapshot({
      language: 'zh-CN',
      theme: 'dark',
      launchOnLogin: false,
      launchOnLoginAvailable: true,
      steamWebApiKey: 'test-key',
      workshopQuery: 'city',
      workshopAgeRatings: ['g', 'pg_13'],
      workshopItemTypes: ['video', 'scene', 'web', 'application'],
      steamRequired: false,
      steamStatusMessage: 'Steam is optional for the current setup.',
      stale: false
    });

    const { body } = render(SettingsPage);

    expect(body).toContain('设置');
    expect(body).toContain('应用偏好');
    expect(body).toContain('当前设置');
    expect(body).toContain('可复制诊断信息');
    expect(body).toContain('编辑设置');
  });

  it('describes launch-on-login as a saved preference when autostart is unavailable', () => {
    setSettingsSnapshot({
      language: 'system',
      theme: 'dark',
      launchOnLogin: true,
      launchOnLoginAvailable: false,
      steamWebApiKey: '',
      workshopQuery: '',
      workshopAgeRatings: ['g', 'pg_13'],
      workshopItemTypes: ['video', 'scene', 'web', 'application'],
      steamRequired: false,
      steamStatusMessage: 'Steam is optional for the current setup.',
      stale: false
    });

    const { body } = render(SettingsPage);
    const { body: editingBody } = render(SettingsPage, { props: { initialEditing: true } });

    expect(editingBody).toContain('Launch-on-login is currently unavailable on this machine.');
    expect(editingBody).toContain('Saved preference: prefer enabled when available.');
    expect(body).toContain('Saved launch preference:');
    expect(body).toContain('Prefer enabled when available');
    expect(editingBody).not.toContain('aria-label="Launch on login"');
    expect(body).not.toContain('Launch on login:');
  });
});
