import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import WorkshopPage from './+page.svelte';
import * as ipc from '$lib/ipc';
import { setLibrarySnapshot, setWorkshopOnlineCache, setWorkshopSnapshot } from '$lib/stores/ui';
import { librarySnapshot, searchResult, workshopSnapshot, outcome } from '$lib/testing/page-fixtures';
import { button, deferred, mountPage, resetPageStores, waitForText } from '$lib/testing/interactions';

vi.mock('$lib/ipc', () => ({
  loadLibraryPage: vi.fn(), loadWorkshopPage: vi.fn(), loadSettingsPage: vi.fn(),
  openWorkshopInSteam: vi.fn(), refreshWorkshopCatalog: vi.fn(), searchWorkshopOnline: vi.fn(), updateSettings: vi.fn()
}));
let page: Awaited<ReturnType<typeof mountPage>>;
beforeEach(() => {
  vi.resetAllMocks(); resetPageStores();
  setWorkshopSnapshot(workshopSnapshot); setLibrarySnapshot(librarySnapshot);
  setWorkshopOnlineCache({ query: 'forest', ageRatings: ['g'], itemTypes: ['video'], pageSize: 24, result: searchResult });
  vi.mocked(ipc.updateSettings).mockResolvedValue(outcome());
  vi.mocked(ipc.searchWorkshopOnline).mockResolvedValue(searchResult);
});
afterEach(async () => { await page?.destroy(); resetPageStores(); });

describe('Workshop search and Steam action interactions', () => {
  it('reports a successful catalog refresh separately when local markers fail, and retries only marker loading', async () => {
    vi.mocked(ipc.refreshWorkshopCatalog).mockResolvedValue(outcome({ message: 'Catalog refreshed', currentUpdate: workshopSnapshot }));
    vi.mocked(ipc.loadLibraryPage).mockRejectedValueOnce(new Error('Markers unavailable')).mockResolvedValueOnce(librarySnapshot);
    page = await mountPage(WorkshopPage);
    button(page.target, 'Refresh Catalog').click();
    await waitForText(page.target, 'The action completed, but the latest data could not be loaded: Markers unavailable');
    expect(page.target.textContent).toContain('Catalog refreshed');
    expect(page.target.textContent).toContain('Forest Online');
    button(page.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Markers unavailable'));
    expect(ipc.refreshWorkshopCatalog).toHaveBeenCalledTimes(1);
    expect(ipc.loadLibraryPage).toHaveBeenCalledTimes(2);
  });

  it.each(['business failure', 'IPC rejection'])('shows %s beside the Steam action and retains search results', async (failure) => {
    const open = vi.mocked(ipc.openWorkshopInSteam);
    if (failure === 'business failure') open.mockResolvedValueOnce(outcome({ ok: false, message: 'Steam unavailable' }));
    else open.mockRejectedValueOnce(new Error('Steam unavailable'));
    open.mockResolvedValueOnce(outcome({ message: 'Opened in Steam' }));
    page = await mountPage(WorkshopPage);
    button(page.target, 'Open In Steam').click();
    await waitForText(page.target, 'Steam unavailable');
    expect(page.target.textContent).toContain('Forest Online');
    expect(page.target.textContent).toContain('Online results');
    expect(page.target.textContent).not.toContain('Showing the last successful results');
    expect(ipc.searchWorkshopOnline).not.toHaveBeenCalled();
    button(page.target, 'Open In Steam').click();
    await waitForText(page.target, 'Opened in Steam');
    expect(open).toHaveBeenCalledTimes(2);
  });

  it('keeps previous results and retries the failed next page request without losing the query', async () => {
    const search = vi.mocked(ipc.searchWorkshopOnline);
    search.mockRejectedValueOnce(new Error('Search unavailable'));
    search.mockResolvedValueOnce({ ...searchResult, page: 3 });
    page = await mountPage(WorkshopPage);
    button(page.target, 'Next').click();
    await waitForText(page.target, 'Search unavailable');
    expect(page.target.textContent).toContain('Forest Online');
    expect(page.target.textContent).toContain('Showing the last successful results');
    expect(page.target.querySelector('input[type="text"]')?.getAttribute('placeholder')).toBe('Search Steam Workshop');
    button(page.target, 'Retry').click();
    await vi.waitFor(() => expect(search).toHaveBeenCalledTimes(2));
    expect(search.mock.calls[0][0]).toMatchObject({ query: 'forest', page: 3 });
    expect(search.mock.calls[1][0]).toMatchObject({ query: 'forest', page: 3 });
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Search unavailable'));
  });

  it('keeps a search error when a separate Steam action also fails', async () => {
    vi.mocked(ipc.searchWorkshopOnline).mockRejectedValue(new Error('Search unavailable'));
    vi.mocked(ipc.openWorkshopInSteam).mockRejectedValue(new Error('Steam unavailable'));
    page = await mountPage(WorkshopPage);
    button(page.target, 'Search').click();
    await waitForText(page.target, 'Search unavailable');
    button(page.target, 'Open In Steam').click();
    await waitForText(page.target, 'Steam unavailable');
    expect(page.target.textContent).toContain('Search unavailable');
    expect(page.target.textContent).toContain('Forest Online');
  });

  it('retains results during a filter change and failed search', async () => {
    const search = deferred<typeof searchResult>();
    vi.mocked(ipc.searchWorkshopOnline).mockReturnValue(search.promise);
    page = await mountPage(WorkshopPage);
    button(page.target, 'Show filters').click();
    await vi.waitFor(() => expect(page.target.querySelector('fieldset')).not.toBeNull());
    const filter = page.target.querySelector('input[type="checkbox"]') as HTMLInputElement;
    filter.click();
    await vi.waitFor(() => expect(ipc.searchWorkshopOnline).toHaveBeenCalled());
    expect(page.target.textContent).toContain('Forest Online');
    search.reject(new Error('Filter search unavailable'));
    await waitForText(page.target, 'Filter search unavailable');
    expect(page.target.textContent).toContain('Forest Online');
  });
});
