import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { tick } from 'svelte';
import SettingsPage from './+page.svelte';
import * as ipc from '$lib/ipc';
import { setSettingsSnapshot } from '$lib/stores/ui';
import { settingsSnapshot, outcome } from '$lib/testing/page-fixtures';
import { button, deferred, mountPage, resetPageStores, waitForText } from '$lib/testing/interactions';
import type { ActionOutcome, SettingsPageSnapshot } from '$lib/types';

vi.mock('$lib/ipc', () => ({ loadDiagnostics: vi.fn(), loadSettingsPage: vi.fn(), updateSettings: vi.fn() }));
let page: Awaited<ReturnType<typeof mountPage>>;
beforeEach(() => { vi.resetAllMocks(); resetPageStores(); setSettingsSnapshot(settingsSnapshot); });
afterEach(async () => { await page?.destroy(); resetPageStores(); });

const editDraft = async () => {
  button(page.target, 'Edit settings').click();
  await tick();
  const field = page.target.querySelector('input[type="password"]') as HTMLInputElement;
  field.value = 'test-draft-key';
  field.dispatchEvent(new Event('input', { bubbles: true }));
  await tick();
  return field;
};

describe('Settings save interactions', () => {
  it('distinguishes a successful save from an unavailable optional refresh and retries only loading', async () => {
    vi.mocked(ipc.updateSettings).mockResolvedValue(outcome({ message: 'Settings saved', invalidations: ['settings'] }));
    vi.mocked(ipc.loadSettingsPage).mockRejectedValueOnce(new Error('Settings refresh unavailable')).mockResolvedValueOnce({ ...settingsSnapshot, steamWebApiKey: 'test-draft-key' });
    page = await mountPage(SettingsPage);
    await editDraft();
    button(page.target, 'Save changes').click();
    await waitForText(page.target, 'The action completed, but the latest data could not be loaded: Settings refresh unavailable');
    expect(page.target.textContent).toContain('Settings saved');
    button(page.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(page.target.textContent).not.toContain('Settings refresh unavailable'));
    expect(ipc.updateSettings).toHaveBeenCalledTimes(1);
    expect(ipc.loadSettingsPage).toHaveBeenCalledTimes(2);
  });

  it.each(['business failure', 'IPC rejection'])('keeps the editable draft after %s and allows retry', async (failure) => {
    const save = vi.mocked(ipc.updateSettings);
    if (failure === 'business failure') save.mockResolvedValueOnce(outcome({ ok: false, message: 'Save denied' }));
    else save.mockRejectedValueOnce(new Error('Save denied'));
    save.mockResolvedValueOnce(outcome({ message: 'Saved', currentUpdate: { ...settingsSnapshot, steamWebApiKey: 'test-draft-key' } }));
    page = await mountPage(SettingsPage);
    await editDraft();
    button(page.target, 'Save changes').click();
    await waitForText(page.target, 'Save denied');
    expect((page.target.querySelector('input[type="password"]') as HTMLInputElement).value).toBe('test-draft-key');
    expect(button(page.target, 'Save changes').disabled).toBe(false);
    button(page.target, 'Save changes').click();
    await waitForText(page.target, 'Saved');
    expect(save).toHaveBeenCalledTimes(2);
    await vi.waitFor(() => expect(page.target.querySelector('input[type="password"]')).toBeNull());
  });

  it('locks every editor while saving and submits only once', async () => {
    const pending = deferred<ActionOutcome<SettingsPageSnapshot>>();
    vi.mocked(ipc.updateSettings).mockReturnValue(pending.promise);
    page = await mountPage(SettingsPage);
    await editDraft();
    button(page.target, 'Save changes').click();
    await tick();
    expect((page.target.querySelector('input[type="password"]') as HTMLInputElement).disabled).toBe(true);
    expect((page.target.querySelector('input[type="checkbox"]') as HTMLInputElement).disabled).toBe(true);
    expect(button(page.target, 'Language').disabled).toBe(true);
    expect(button(page.target, 'Theme').disabled).toBe(true);
    button(page.target, 'Saving…').click();
    expect(ipc.updateSettings).toHaveBeenCalledTimes(1);
    pending.resolve(outcome({ currentUpdate: { ...settingsSnapshot, steamWebApiKey: 'test-draft-key' } }));
    await vi.waitFor(() => expect(page.target.querySelector('input[type="password"]')).toBeNull());
  });
});
