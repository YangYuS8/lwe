import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import DesktopPage from './+page.svelte';
import * as ipc from '$lib/ipc';
import { setDesktopSnapshot, setPageStale } from '$lib/stores/ui';
import { desktopSnapshot, outcome } from '$lib/testing/page-fixtures';
import { button, mountPage, resetPageStores, waitForText } from '$lib/testing/interactions';

vi.mock('$lib/ipc', () => ({ clearLibraryItemFromMonitor: vi.fn(), loadDesktopPage: vi.fn() }));
let page: Awaited<ReturnType<typeof mountPage>>;
beforeEach(() => {
  vi.resetAllMocks(); resetPageStores(); setDesktopSnapshot(desktopSnapshot);
  vi.mocked(ipc.loadDesktopPage).mockResolvedValue(desktopSnapshot);
});
afterEach(async () => { await page?.destroy(); resetPageStores(); });

describe('Desktop action interactions', () => {
  it('keeps cached monitor cards after a page load failure and retries without clearing', async () => {
    setPageStale('desktop');
    vi.mocked(ipc.loadDesktopPage).mockRejectedValueOnce(new Error('Initial load unavailable'));
    page = await mountPage(DesktopPage);
    await waitForText(page.target, 'Initial load unavailable');
    expect(page.target.textContent).toContain('Forest Video');
    button(page.target, 'Retry').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Initial load unavailable'));
    expect(ipc.loadDesktopPage).toHaveBeenCalledTimes(2);
    expect(ipc.clearLibraryItemFromMonitor).not.toHaveBeenCalled();
  });

  it.each(['business failure', 'IPC rejection'])('keeps monitor cards and enables clear retry after %s', async (failure) => {
    const clear = vi.mocked(ipc.clearLibraryItemFromMonitor);
    if (failure === 'business failure') clear.mockResolvedValueOnce(outcome({ ok: false, message: 'Clear denied', invalidations: ['desktop'] }));
    else clear.mockRejectedValueOnce(new Error('Clear denied'));
    clear.mockResolvedValueOnce(outcome({ message: 'Cleared', invalidations: ['desktop'] }));
    page = await mountPage(DesktopPage);
    button(page.target, 'Clear wallpaper from Primary').click();
    await waitForText(page.target, 'Clear denied');
    expect(page.target.textContent).toContain('Forest Video');
    expect(ipc.loadDesktopPage).not.toHaveBeenCalled();
    expect(button(page.target, 'Clear wallpaper from Primary').disabled).toBe(false);
    button(page.target, 'Clear wallpaper from Primary').click();
    await waitForText(page.target, 'Cleared');
    expect(clear).toHaveBeenCalledTimes(2);
  });

  it('keeps successful clear feedback and reloads without clearing again after refresh failure', async () => {
    vi.mocked(ipc.clearLibraryItemFromMonitor).mockResolvedValue(outcome({ message: 'Cleared', invalidations: ['desktop'] }));
    vi.mocked(ipc.loadDesktopPage).mockRejectedValueOnce(new Error('Monitor data unavailable'));
    page = await mountPage(DesktopPage);
    button(page.target, 'Clear wallpaper from Primary').click();
    await waitForText(page.target, 'The action completed, but the latest data could not be loaded: Monitor data unavailable');
    expect(page.target.textContent).toContain('Cleared');
    expect(page.target.textContent).toContain('Forest Video');
    button(page.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Monitor data unavailable'));
    expect(ipc.clearLibraryItemFromMonitor).toHaveBeenCalledTimes(1);
    expect(ipc.loadDesktopPage).toHaveBeenCalledTimes(2);
  });
});
