import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import LibraryPage from './+page.svelte';
import * as ipc from '$lib/ipc';
import { pageCache, setDesktopSnapshot, setLibraryDetail, setLibrarySnapshot, setPageStale } from '$lib/stores/ui';
import { librarySnapshot, libraryDetail, desktopSnapshot, settingsSnapshot, outcome } from '$lib/testing/page-fixtures';
import { button, mountPage, resetPageStores, waitForText } from '$lib/testing/interactions';

vi.mock('$lib/ipc', () => ({
  applyLibraryItemToMonitor: vi.fn(), loadDesktopPage: vi.fn(), loadLibraryItemDetail: vi.fn(),
  loadLibraryPage: vi.fn(), loadSettingsPage: vi.fn(), refreshWorkshopCatalog: vi.fn(), updateSettings: vi.fn()
}));

let page: Awaited<ReturnType<typeof mountPage>>;
beforeEach(() => {
  vi.resetAllMocks();
  resetPageStores();
  setLibrarySnapshot(librarySnapshot);
  setLibraryDetail(libraryDetail);
  setDesktopSnapshot(desktopSnapshot);
  vi.mocked(ipc.loadSettingsPage).mockResolvedValue(settingsSnapshot);
  vi.mocked(ipc.loadLibraryPage).mockResolvedValue({ ...librarySnapshot, selectedItemId: null });
  vi.mocked(ipc.loadLibraryItemDetail).mockResolvedValue(libraryDetail);
  vi.mocked(ipc.loadDesktopPage).mockResolvedValue(desktopSnapshot);
});
afterEach(async () => { await page?.destroy(); resetPageStores(); });

describe('Library action interactions', () => {
  it('keeps the cached page and detail after a page load failure and retries without changing the selection', async () => {
    setPageStale('library');
    vi.mocked(ipc.loadLibraryPage).mockRejectedValueOnce(new Error('Initial load unavailable'));
    page = await mountPage(LibraryPage);
    await waitForText(page.target, 'Initial load unavailable');
    expect(page.target.querySelector('[data-detail-section="metadata"]')).not.toBeNull();
    button(page.target, 'Retry').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Initial load unavailable'));
    expect(get(pageCache).library.snapshot?.selectedItemId).toBe('video-7');
    expect(ipc.applyLibraryItemToMonitor).not.toHaveBeenCalled();
  });

  it('keeps the loaded detail when the apply succeeds but detail refresh fails', async () => {
    vi.mocked(ipc.applyLibraryItemToMonitor).mockResolvedValue(outcome({ message: 'Applied', invalidations: ['library', 'desktop'] }));
    vi.mocked(ipc.loadLibraryItemDetail).mockRejectedValueOnce(new Error('Detail refresh unavailable'));
    page = await mountPage(LibraryPage);
    button(page.target, 'Apply').click();
    await waitForText(page.target, 'The action completed, but the latest data could not be loaded: Detail refresh unavailable');
    expect(page.target.querySelector('[data-detail-section="metadata"]')).not.toBeNull();
    button(page.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Detail refresh unavailable'));
    expect(ipc.applyLibraryItemToMonitor).toHaveBeenCalledTimes(1);
  });

  it.each(['business failure', 'IPC rejection'])('keeps the selected detail and enables apply retry after %s', async (failure) => {
    const apply = vi.mocked(ipc.applyLibraryItemToMonitor);
    if (failure === 'business failure') apply.mockResolvedValueOnce(outcome({ ok: false, message: 'Apply denied', invalidations: ['library', 'desktop'] }));
    else apply.mockRejectedValueOnce(new Error('Apply denied'));
    apply.mockResolvedValueOnce(outcome({ message: 'Applied', invalidations: ['library', 'desktop'] }));
    page = await mountPage(LibraryPage);
    button(page.target, 'Apply').click();
    await waitForText(page.target, 'Apply denied');
    expect(page.target.querySelector('[data-detail-section="metadata"]')?.textContent).toContain('A forest wallpaper.');
    expect(Array.from(page.target.querySelectorAll('[role="status"]')).map((element) => element.textContent).join(' ')).not.toContain('Apply denied');
    expect(ipc.loadLibraryPage).not.toHaveBeenCalled();
    if (failure === 'business failure') {
      expect(get(pageCache).library.stale).toBe(true);
      expect(get(pageCache).desktop.stale).toBe(true);
      expect(get(pageCache).library.detail).toEqual(libraryDetail);
    }
    expect(button(page.target, 'Apply').disabled).toBe(false);
    button(page.target, 'Apply').click();
    await waitForText(page.target, 'Applied');
    await vi.waitFor(() => expect(ipc.loadLibraryItemDetail).toHaveBeenCalledWith('video-7'));
    expect(apply).toHaveBeenCalledTimes(2);
    expect(get(pageCache).library.snapshot?.selectedItemId).toBe('video-7');
  });

  it('reports a successful apply separately from failed refresh and retries only data loading', async () => {
    vi.mocked(ipc.applyLibraryItemToMonitor).mockResolvedValue(outcome({ message: 'Applied', invalidations: ['library', 'desktop'] }));
    vi.mocked(ipc.loadLibraryPage).mockRejectedValueOnce(new Error('Catalog unavailable'));
    page = await mountPage(LibraryPage);
    button(page.target, 'Apply').click();
    await waitForText(page.target, 'The action completed, but the latest data could not be loaded: Catalog unavailable');
    expect(page.target.textContent).toContain('Applied');
    expect(page.target.querySelector('[data-detail-section="metadata"]')).not.toBeNull();
    expect(get(pageCache).library.stale).toBe(true);
    button(page.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(ipc.loadLibraryPage).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Catalog unavailable'));
    expect(ipc.applyLibraryItemToMonitor).toHaveBeenCalledTimes(1);
    expect(get(pageCache).library.snapshot?.selectedItemId).toBe('video-7');
  });

  it('preserves loaded detail when reloading it fails and retries in place', async () => {
    vi.mocked(ipc.loadLibraryItemDetail).mockRejectedValueOnce(new Error('Detail unavailable'));
    page = await mountPage(LibraryPage);
    button(page.target, 'Select Library item Forest Video').click();
    await waitForText(page.target, 'Detail unavailable');
    expect(page.target.querySelector('[data-detail-section="metadata"]')?.textContent).toContain('A forest wallpaper.');
    button(page.target, 'Retry').click();
    await vi.waitFor(() => expect(ipc.loadLibraryItemDetail).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Detail unavailable'));
  });
});
