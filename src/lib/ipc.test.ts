import { afterEach, describe, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn().mockResolvedValue({ ok: false, message: 'unavailable' })
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke
}));

import {
  applyLibraryItemToMonitor,
  clearLibraryItemFromMonitor,
  loadLibraryPage,
  searchWorkshopOnline,
  updateSettings
} from './ipc';

describe('ipc desktop flow bridge', () => {
  afterEach(() => {
    invoke.mockClear();
  });

  it('invokes the desktop apply and clear commands with the active flow names', async () => {
    await applyLibraryItemToMonitor('DISPLAY-1', 'item-1');
    await clearLibraryItemFromMonitor('DISPLAY-1');

    expect(invoke).toHaveBeenNthCalledWith(1, 'apply_library_item_to_monitor', {
      monitorId: 'DISPLAY-1',
      itemId: 'item-1'
    });
    expect(invoke).toHaveBeenNthCalledWith(2, 'clear_library_item_from_monitor', {
      monitorId: 'DISPLAY-1'
    });
  });
});

describe('ipc request errors', () => {
  it('preserves a Rust string rejection as an Error for page diagnostics', async () => {
    invoke.mockRejectedValueOnce('Failed to read libraryfolders.vdf');
    await expect(loadLibraryPage()).rejects.toThrow('Failed to read libraryfolders.vdf');
  });

  it('preserves an existing Error instance', async () => {
    const failure = new Error('IPC transport unavailable');
    invoke.mockRejectedValueOnce(failure);
    await expect(loadLibraryPage()).rejects.toBe(failure);
  });
});

describe('ipc settings flow bridge', () => {
  afterEach(() => {
    invoke.mockClear();
  });

  it('invokes the settings update command with the editable mvp payload', async () => {
    await updateSettings({
      language: 'en',
      theme: 'dark',
      launchOnLogin: true,
      steamWebApiKey: 'test-key',
      workshopQuery: 'forest',
      workshopAgeRatings: ['g', 'pg_13'],
      workshopItemTypes: ['video', 'application']
    });

    expect(invoke).toHaveBeenCalledWith('update_settings', {
      input: {
        language: 'en',
        theme: 'dark',
        launchOnLogin: true,
        steamWebApiKey: 'test-key',
        workshopQuery: 'forest',
        workshopAgeRatings: ['g', 'pg_13'],
        workshopItemTypes: ['video', 'application']
      }
    });
  });

  it('invokes the online workshop search command with filters', async () => {
    await searchWorkshopOnline({
      query: 'neon',
      ageRatings: ['g', 'pg_13'],
      itemTypes: ['video', 'scene'],
      page: 2,
      pageSize: 48
    });

    expect(invoke).toHaveBeenCalledWith('search_workshop_online', {
      input: {
        query: 'neon',
        ageRatings: ['g', 'pg_13'],
        itemTypes: ['video', 'scene'],
        page: 2,
        pageSize: 48
      }
    });
  });
});
