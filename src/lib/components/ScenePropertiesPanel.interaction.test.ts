import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { tick } from 'svelte';
import { createClassComponent } from 'svelte/legacy';
import ScenePropertiesPanel from './ScenePropertiesPanel.svelte';
import * as ipc from '$lib/ipc';
import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import { button, deferred, waitForText } from '$lib/testing/interactions';
import { outcome } from '$lib/testing/page-fixtures';
import type { ActionOutcome, ScenePropertiesSnapshot } from '$lib/types';

vi.mock('$lib/ipc', () => ({ chooseSceneImage: vi.fn(), loadSceneProperties: vi.fn(), updateSceneProperties: vi.fn() }));

const fixture = (monitorId = 'DP-1', itemId = 'scene-1'): ScenePropertiesSnapshot => ({
  itemId, monitorId,
  groups: [],
  properties: [
    { name: 'hdr', label: 'Author HDR label', kind: 'bool', value: true, min: null, max: null, step: null, order: 0 },
    { name: 'strength', label: 'Author brightness', kind: 'slider', value: 1.5, min: 0, max: 5, step: 0.5, order: 1 },
    { name: 'tint', label: '作者颜色', kind: 'color', value: [0.25, 0.5, 1], min: null, max: null, step: null, order: 2 }
  ]
});

const groupedFixture = (monitorId = 'DP-1', itemId = 'scene-1'): ScenePropertiesSnapshot => ({
  ...fixture(monitorId, itemId),
  properties: [
    ...fixture().properties,
    { name: 'selection', label: 'Author background', kind: 'combo', value: 'first',
      options: [{ label: 'First image', value: 'first' }, { label: 'Second image', value: 'second' }],
      min: null, max: null, step: null, order: 6 }
  ],
  groups: [
    { name: 'appearance', label: 'Author visuals', order: 0.5, members: ['strength', 'tint'] },
    { name: 'spacing', label: '', order: 3, members: [] },
    { name: 'choices', label: 'Author visuals', order: 5, members: ['selection'] }
  ]
});

const onOutcome = vi.fn<(value: ActionOutcome<null>) => void>();
const imageToken = 'sha256:' + 'a'.repeat(64);
const imageFixture = (monitorId = 'DP-1'): ScenePropertiesSnapshot => ({
  ...fixture(monitorId), properties: [...fixture().properties, {
    name: 'background', label: 'Author image control', kind: 'texture', value: null,
    min: null, max: null, step: null, order: 3
  }]
});
const authorNote = '<img src="https://example.invalid/banner.png">\n<a href="https://example.invalid">Author note</a>';
const textFixture = (): ScenePropertiesSnapshot => ({
  ...fixture(), properties: [...fixture().properties,
    { name: 'note', label: authorNote, kind: 'text_display', value: null, min: null, max: null, step: null, order: 3 },
    { name: 'caption', label: 'Author caption', kind: 'text_input', value: 'Default caption',
      min: null, max: null, step: null, order: 4 }
  ],
  groups: [{ name: 'text', label: 'Author text', order: 2.5, members: ['note', 'caption'] }]
});
let panel: Awaited<ReturnType<typeof mountPanel>> | undefined;

async function mountPanel(disabled = false) {
  const target = document.createElement('div');
  document.body.append(target);
  const instance = createClassComponent({
    component: ScenePropertiesPanel, target, props: { itemId: 'scene-1', monitorId: 'DP-1', disabled, onOutcome }
  });
  await tick();
  return {
    target,
    set: instance.$set.bind(instance),
    destroy: () => { instance.$destroy(); target.remove(); }
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  resetPreferredLanguage();
  vi.mocked(ipc.loadSceneProperties).mockResolvedValue(fixture());
});
afterEach(async () => { panel?.destroy(); panel = undefined; await tick(); resetPreferredLanguage(); });

const open = async () => {
  button(panel!.target, 'Scene properties').click();
  await waitForText(panel!.target, 'Author brightness');
};
const field = (type: string) => panel!.target.querySelector<HTMLInputElement>(`input[type="${type}"]`)!;
const edit = async (type: string, value: string) => {
  const input = field(type);
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await tick();
};

describe('ScenePropertiesPanel interactions', () => {
  it('renders author display text inertly and saves literal captions including an empty value', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(textFixture());
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome({ ok: false, message: 'Preparation rejected' }));
    panel = await mountPanel(); await open();
    const group = panel.target.querySelector('[data-scene-property-group="text"]')!;
    const caption = group.querySelector('textarea')!;
    expect(group.textContent).toContain(authorNote);
    expect(group.querySelector('img, a')).toBeNull();
    expect(group.querySelector(`label[for="${caption.id}"]`)?.textContent).toBe('Author caption');
    expect(caption.value).toBe('Default caption');
    const text = ' 你好\n<caption> ';
    for (const value of [text, '']) {
      caption.value = value;
      caption.dispatchEvent(new Event('input', { bubbles: true }));
      await tick();
      button(panel.target, 'Save and apply').click();
      await waitForText(panel.target, 'Preparation rejected');
      expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', {
        hdr: true, strength: 1.5, tint: [0.25, 0.5, 1], caption: value
      });
      expect(caption.value).toBe(value);
    }
    expect(ipc.updateSceneProperties).toHaveBeenCalledTimes(2);
  });

  it('enforces UTF-8 byte and line limits for text drafts and rejects control characters', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(textFixture());
    panel = await mountPanel(); await open();
    const caption = panel.target.querySelector('textarea')!;
    for (const value of ['🌙'.repeat(65), '\n'.repeat(8), 'text\u0085']) {
      caption.value = value;
      caption.dispatchEvent(new Event('input', { bubbles: true }));
      await tick();
      expect(caption.getAttribute('aria-invalid')).toBe('true');
      expect(button(panel.target, 'Save and apply').disabled).toBe(true);
    }
    caption.value = '🌙'.repeat(62) + '\n'.repeat(7) + '!';
    caption.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    expect(caption.getAttribute('aria-invalid')).toBe('false');
    expect(button(panel.target, 'Save and apply').disabled).toBe(false);
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('imports an image into the draft, preserves it on cancellation and failed save, and clears back to the author default', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(imageFixture());
    vi.mocked(ipc.chooseSceneImage).mockResolvedValueOnce({ token: imageToken, width: 3840, height: 2160 }).mockResolvedValueOnce(null);
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome({ ok: false, message: 'Preparation rejected' }));
    panel = await mountPanel(); await open();
    button(panel.target, 'Choose image').click();
    await waitForText(panel.target, 'Custom image selected');
    expect(ipc.chooseSceneImage).toHaveBeenCalledExactlyOnceWith('scene-1', 'background');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
    button(panel.target, 'Choose image').click(); await vi.waitFor(() => expect(ipc.chooseSceneImage).toHaveBeenCalledTimes(2)); await tick();
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Preparation rejected');
    expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', {
      hdr: true, strength: 1.5, tint: [0.25, 0.5, 1], background: imageToken
    });
    expect(panel.target.textContent).toContain('Custom image selected');
    button(panel.target, 'Use author image').click(); await tick();
    button(panel.target, 'Save and apply').click();
    await vi.waitFor(() => expect(ipc.updateSceneProperties).toHaveBeenCalledTimes(2));
    expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', { hdr: true, strength: 1.5, tint: [0.25, 0.5, 1] });
  });

  it('ignores an image import that completes after switching monitors', async () => {
    const pending = deferred<Awaited<ReturnType<typeof ipc.chooseSceneImage>>>();
    vi.mocked(ipc.loadSceneProperties).mockResolvedValueOnce(imageFixture()).mockResolvedValueOnce(imageFixture('DP-2'));
    vi.mocked(ipc.chooseSceneImage).mockReturnValue(pending.promise);
    panel = await mountPanel(); await open(); button(panel.target, 'Choose image').click(); await tick();
    expect(button(panel.target, 'Save and apply').disabled).toBe(true);
    panel.set({ monitorId: 'DP-2' }); await tick(); await open();
    pending.resolve({ token: imageToken, width: 2, height: 2 }); await tick();
    expect(panel.target.textContent).not.toContain('Custom image selected');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('rejects imported URLs and invalid metadata without replacing the author image', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(imageFixture());
    vi.mocked(ipc.chooseSceneImage).mockResolvedValue({ token: 'file:///external.png', width: 2, height: 2 });
    panel = await mountPanel(); await open(); button(panel.target, 'Choose image').click();
    await waitForText(panel.target, 'Invalid image selection.');
    expect(panel.target.textContent).not.toContain('Custom image selected');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('renders author-ordered sections, keeps preceding controls ungrouped, and allows duplicate or blank labels', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(groupedFixture());
    panel = await mountPanel();
    await open();
    const groups = [...panel.target.querySelectorAll<HTMLElement>('[data-scene-property-group]')];
    expect(groups.map((group) => group.dataset.scenePropertyGroup)).toEqual(['appearance', 'spacing', 'choices']);
    expect(field('checkbox').closest('[data-scene-property-group]')).toBeNull();
    expect(field('number').closest('[data-scene-property-group]')).toBe(groups[0]);
    expect(field('color').closest('[data-scene-property-group]')).toBe(groups[0]);
    expect(panel.target.querySelector('select')!.closest('[data-scene-property-group]')).toBe(groups[2]);
    expect(groups.map((group) => group.querySelector('button')!.textContent?.replace('−', '').trim()))
      .toEqual(['Author visuals', 'Property group 2', 'Author visuals']);
    expect(groups[1].textContent).toContain('This group has no editable properties');
    const controls = [...panel.target.querySelectorAll('input, select')];
    expect(new Set(controls.map((control) => control.id)).size).toBe(4);
    expect(controls.every((control) => panel!.target.querySelector(`label[for="${control.id}"]`))).toBe(true);
  });

  it('uses focusable disclosure buttons with connected content and preserves a hidden draft after collapse', async () => {
    const snapshot = groupedFixture();
    snapshot.groups[0].name = '__proto__';
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(snapshot);
    panel = await mountPanel();
    await open();
    await edit('number', '3.25');
    const header = panel.target.querySelector<HTMLButtonElement>('[data-scene-property-group] button')!;
    const content = panel.target.querySelector<HTMLElement>(`[id="${header.getAttribute('aria-controls')}"]`)!;
    expect(header.tagName).toBe('BUTTON');
    expect(header.type).toBe('button');
    expect(header.tabIndex).toBe(0);
    expect(header.getAttribute('aria-expanded')).toBe('true');
    expect(content.hidden).toBe(false);
    header.focus();
    header.click();
    await tick();
    expect(document.activeElement).toBe(header);
    expect(header.getAttribute('aria-expanded')).toBe('false');
    expect(content.hidden).toBe(true);
    expect(field('number').value).toBe('3.25');
    header.click();
    await tick();
    expect(header.getAttribute('aria-expanded')).toBe('true');
    expect(content.hidden).toBe(false);
    expect(field('number').value).toBe('3.25');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('saves only values from collapsed groups, retains failed edits, retries, and resets without changing disclosure state', async () => {
    const initial = groupedFixture();
    const saved = groupedFixture();
    saved.properties[1] = { ...saved.properties[1], kind: 'slider', value: 3.25 };
    vi.mocked(ipc.loadSceneProperties).mockResolvedValueOnce(initial).mockResolvedValueOnce(saved).mockResolvedValueOnce(initial);
    vi.mocked(ipc.updateSceneProperties).mockResolvedValueOnce(outcome({ ok: false, message: 'Preparation rejected' }))
      .mockResolvedValueOnce(outcome()).mockResolvedValueOnce(outcome());
    panel = await mountPanel();
    await open();
    await edit('number', '3.25');
    const header = panel.target.querySelector<HTMLButtonElement>('[data-scene-property-group] button')!;
    header.click();
    await tick();
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Preparation rejected');
    expect(header.getAttribute('aria-expanded')).toBe('false');
    expect(field('number').value).toBe('3.25');
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
    expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', {
      hdr: true, strength: 3.25, tint: [0.25, 0.5, 1], selection: 'first'
    });
    button(panel.target, 'Save and apply').click();
    await vi.waitFor(() => expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(button(panel!.target, 'Reset defaults').disabled).toBe(false));
    expect(header.getAttribute('aria-expanded')).toBe('false');
    button(panel.target, 'Reset defaults').click();
    await vi.waitFor(() => expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', {}));
    await vi.waitFor(() => expect(field('number').value).toBe('1.5'));
    expect(header.getAttribute('aria-expanded')).toBe('false');
  });

  it('locks section disclosures during pending application and resets them when selecting another monitor', async () => {
    const pending = deferred<ActionOutcome<null>>();
    vi.mocked(ipc.loadSceneProperties).mockResolvedValueOnce(groupedFixture()).mockResolvedValueOnce(groupedFixture('DP-2'));
    vi.mocked(ipc.updateSceneProperties).mockReturnValue(pending.promise);
    panel = await mountPanel();
    await open();
    const header = panel.target.querySelector<HTMLButtonElement>('[data-scene-property-group] button')!;
    header.click();
    await tick();
    button(panel.target, 'Save and apply').click();
    await tick();
    expect(header.disabled).toBe(true);
    header.click();
    await tick();
    expect(header.getAttribute('aria-expanded')).toBe('false');
    panel.set({ monitorId: 'DP-2' });
    await tick();
    await open();
    expect(panel.target.querySelector('[data-scene-property-group] button')!.getAttribute('aria-expanded')).toBe('true');
    pending.resolve(outcome());
    await tick();
    expect(ipc.loadSceneProperties).toHaveBeenLastCalledWith('DP-2', 'scene-1');
  });

  it.each(['missing groups', 'wrong label', 'wrong order', 'unknown member', 'repeated member', 'overlapping groups', 'duplicate group', 'value name', 'wrong members'])('rejects malformed group metadata: %s', async (kind) => {
    const invalid = groupedFixture() as unknown as { groups?: Record<string, unknown>[] };
    if (kind === 'missing groups') delete invalid.groups;
    else if (kind === 'wrong label') invalid.groups![0].label = 1;
    else if (kind === 'wrong order') invalid.groups![0].order = 'first';
    else if (kind === 'unknown member') invalid.groups![0].members = ['missing'];
    else if (kind === 'repeated member') invalid.groups![0].members = ['strength', 'strength'];
    else if (kind === 'overlapping groups') invalid.groups![2].members = ['strength'];
    else if (kind === 'duplicate group') invalid.groups![2].name = 'appearance';
    else if (kind === 'value name') invalid.groups![0].name = 'hdr';
    else invalid.groups![0].members = 'strength';
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(invalid as unknown as ScenePropertiesSnapshot);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Invalid scene property data');
    expect(panel.target.querySelector('input')).toBeNull();
    expect(panel.target.querySelector('[data-scene-property-group]')).toBeNull();
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it.each([false, true])('renders non-value-only groups without offering an empty save (display text: %s)', async (displayText) => {
    const snapshot = textFixture();
    snapshot.properties = displayText ? snapshot.properties.filter((property) => property.kind === 'text_display') : [];
    snapshot.groups = [{ ...snapshot.groups[0], label: '', members: displayText ? ['note'] : [] }];
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(snapshot);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Property group 1');
    if (displayText) expect(panel.target.textContent).toContain(authorNote);
    expect(panel.target.textContent).toContain('This scene has no supported editable properties');
    expect(panel.target.textContent).not.toContain('Save and apply');
    expect(panel.target.textContent).not.toContain('Reset defaults');
    expect(panel.target.querySelector('input, textarea')).toBeNull();
  });

  it('edits author combo options as strings and preserves the selection after a failed save', async () => {
    const snapshot = fixture();
    snapshot.properties.push({ name: 'selection', label: 'Author background', kind: 'combo', value: 'first',
      options: [{ label: 'First image', value: 'first' }, { label: 'Second image', value: 'second' }],
      min: null, max: null, step: null, order: 3 });
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(snapshot);
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome({ ok: false, message: 'Preparation rejected' }));
    panel = await mountPanel();
    await open();
    const select = panel.target.querySelector('select')!;
    expect(select.value).toBe('first');
    expect(select.labels?.[0].textContent).toContain('Author background');
    expect([...select.options].map((option) => [option.textContent, option.value])).toEqual([
      ['First image', 'first'], ['Second image', 'second']
    ]);
    select.value = 'second';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Preparation rejected');
    expect(select.value).toBe('second');
    expect(ipc.updateSceneProperties).toHaveBeenCalledWith('DP-1', 'scene-1', {
      hdr: true, strength: 1.5, tint: [0.25, 0.5, 1], selection: 'second'
    });
    expect(button(panel.target, 'Save and apply').disabled).toBe(false);
  });

  it.each(['empty options', 'unknown value', 'duplicate values', 'numeric value', 'missing label'])('rejects malformed combo data: %s', async (kind) => {
    const snapshot = fixture();
    const combo: Record<string, unknown> = { name: 'selection', label: 'Selection', kind: 'combo', value: 'first',
      options: [{ label: 'First', value: 'first' }], min: null, max: null, step: null, order: 3 };
    if (kind === 'empty options') combo.options = [];
    if (kind === 'unknown value') combo.value = 'other';
    if (kind === 'duplicate values') combo.options = [{ label: 'First', value: 'first' }, { label: 'Duplicate', value: 'first' }];
    if (kind === 'numeric value') combo.value = 1;
    if (kind === 'missing label') combo.options = [{ value: 'first' }];
    snapshot.properties.push(combo as unknown as ScenePropertiesSnapshot['properties'][number]);
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(snapshot);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Invalid scene property data');
    expect(panel.target.querySelector('select')).toBeNull();
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('loads lazily and preserves author labels, declared numeric limits and normalized RGB values', async () => {
    panel = await mountPanel();
    expect(ipc.loadSceneProperties).not.toHaveBeenCalled();
    expect(button(panel.target, 'Scene properties').getAttribute('aria-expanded')).toBe('false');
    await open();
    expect(ipc.loadSceneProperties).toHaveBeenCalledExactlyOnceWith('DP-1', 'scene-1');
    expect(field('checkbox').checked).toBe(true);
    expect(field('number').value).toBe('1.5');
    expect(field('number').min).toBe('0');
    expect(field('number').max).toBe('5');
    expect(field('number').step).toBe('0.5');
    expect(field('color').value).toBe('#4080ff');
    expect(panel.target.textContent).toContain('作者颜色');
    expect(panel.target.textContent).toContain('selected monitor');
    button(panel.target, 'Scene properties').click();
    await tick();
    await open();
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
  });

  it.each(['business failure', 'IPC rejection'])('retains all edited values after %s and permits retry', async (failure) => {
    if (failure === 'business failure') vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome({ ok: false, message: 'Preparation rejected' }));
    else vi.mocked(ipc.updateSceneProperties).mockRejectedValue(new Error('Preparation rejected'));
    panel = await mountPanel();
    await open();
    await edit('number', '2.25'); // A declared step is an editor increment, not a quantization rule.
    await edit('color', '#123456');
    field('checkbox').checked = false;
    field('checkbox').dispatchEvent(new Event('change', { bubbles: true }));
    await tick();
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Preparation rejected');
    expect(field('number').value).toBe('2.25');
    expect(field('color').value).toBe('#123456');
    expect(field('checkbox').checked).toBe(false);
    expect(button(panel.target, 'Save and apply').disabled).toBe(false);
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
    expect(ipc.updateSceneProperties).toHaveBeenCalledWith('DP-1', 'scene-1', {
      hdr: false, strength: 2.25, tint: [18 / 255, 52 / 255, 86 / 255]
    });
    expect(panel.target.textContent).not.toContain('Properties saved');
    expect(onOutcome).toHaveBeenCalledTimes(failure === 'business failure' ? 1 : 0);
  });

  it.each(['empty', 'too low', 'too high'])('blocks invalid numeric input: %s', async (kind) => {
    panel = await mountPanel();
    await open();
    await edit('number', kind === 'empty' ? '' : kind === 'too low' ? '-1' : '6');
    expect(button(panel.target, 'Save and apply').disabled).toBe(true);
    button(panel.target, 'Save and apply').click();
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
    expect(panel.target.textContent).toContain('Enter valid values');
    await edit('number', '2.25');
    expect(button(panel.target, 'Save and apply').disabled).toBe(false);
  });

  it.each(['wrong boolean', 'wrong RGB', 'duplicate name', 'wrong monitor', 'invalid limits', 'unknown kind'])('rejects a malformed snapshot: %s', async (kind) => {
    const invalid = fixture() as unknown as { monitorId: string; properties: Record<string, unknown>[] };
    if (kind === 'wrong boolean') invalid.properties[0].value = 1;
    if (kind === 'wrong RGB') invalid.properties[2].value = [1, 2];
    if (kind === 'duplicate name') invalid.properties[1].name = 'hdr';
    if (kind === 'wrong monitor') invalid.monitorId = 'DP-2';
    if (kind === 'invalid limits') invalid.properties[1].min = 6;
    if (kind === 'unknown kind') invalid.properties[0].kind = 'script';
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(invalid as unknown as ScenePropertiesSnapshot);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Invalid scene property data');
    expect(panel.target.querySelector('input')).toBeNull();
    expect(panel.target.textContent).not.toContain('Save and apply');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
  });

  it('uses a successful ActionOutcome and reloads the saved values, preserving unedited color precision', async () => {
    const saved = fixture();
    saved.properties[1] = { ...saved.properties[1], kind: 'slider', value: 3 };
    vi.mocked(ipc.loadSceneProperties).mockResolvedValueOnce(fixture()).mockResolvedValueOnce(saved);
    const result = outcome({ message: 'Scene updated', invalidations: ['library', 'desktop'] });
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(result);
    panel = await mountPanel();
    await open();
    await edit('number', '3');
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Scene updated');
    await vi.waitFor(() => expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(2));
    expect(field('number').value).toBe('3');
    expect(ipc.updateSceneProperties).toHaveBeenCalledWith('DP-1', 'scene-1', { hdr: true, strength: 3, tint: [0.25, 0.5, 1] });
    expect(onOutcome).toHaveBeenCalledExactlyOnceWith(result);
  });

  it('resets by sending an empty override map and reloads author defaults', async () => {
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome());
    panel = await mountPanel();
    await open();
    await edit('number', '4');
    button(panel.target, 'Reset defaults').click();
    await vi.waitFor(() => expect(ipc.updateSceneProperties).toHaveBeenCalledExactlyOnceWith('DP-1', 'scene-1', {}));
    await vi.waitFor(() => expect(field('number').value).toBe('1.5'));
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(2);
  });

  it('recovers invalid saved choices by resetting without a loaded snapshot and reloading default controls', async () => {
    vi.mocked(ipc.loadSceneProperties).mockRejectedValueOnce(new Error('Saved slider is outside updated author range'));
    const result = outcome({ message: 'Defaults applied', invalidations: ['library', 'desktop'] });
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(result);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Saved slider is outside updated author range');
    expect(panel.target.querySelector('input')).toBeNull();
    expect(panel.target.textContent).not.toContain('Save and apply');
    button(panel.target, 'Reset defaults').click();
    await waitForText(panel.target, 'Author brightness');
    expect(ipc.updateSceneProperties).toHaveBeenCalledExactlyOnceWith('DP-1', 'scene-1', {});
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(2);
    expect(field('number').value).toBe('1.5');
    expect(field('checkbox').checked).toBe(true);
    expect(panel.target.textContent).not.toContain('Saved slider is outside updated author range');
    expect(panel.target.textContent).toContain('Defaults applied');
    expect(onOutcome).toHaveBeenCalledExactlyOnceWith(result);
  });

  it.each(['business failure', 'IPC rejection'])('keeps a failed recovery reset visible and retryable after %s', async (failure) => {
    vi.mocked(ipc.loadSceneProperties).mockRejectedValueOnce(new Error('Saved property no longer exists'));
    if (failure === 'business failure') vi.mocked(ipc.updateSceneProperties).mockResolvedValueOnce(outcome({ ok: false, message: 'Defaults are unsupported' }));
    else vi.mocked(ipc.updateSceneProperties).mockRejectedValueOnce(new Error('Defaults are unsupported'));
    vi.mocked(ipc.updateSceneProperties).mockResolvedValueOnce(outcome());
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Saved property no longer exists');
    button(panel.target, 'Reset defaults').click();
    await waitForText(panel.target, 'Defaults are unsupported');
    expect(panel.target.querySelector('input')).toBeNull();
    expect(button(panel.target, 'Reset defaults').disabled).toBe(false);
    expect(panel.target.textContent).not.toContain('Properties saved');
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
    button(panel.target, 'Reset defaults').click();
    await waitForText(panel.target, 'Author brightness');
    expect(ipc.updateSceneProperties).toHaveBeenCalledTimes(2);
    expect(ipc.updateSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-1', {});
    expect(panel.target.textContent).not.toContain('Defaults are unsupported');
  });

  it('blocks recovery reset while disabled or retry loading without a snapshot', async () => {
    vi.mocked(ipc.loadSceneProperties).mockRejectedValueOnce(new Error('Saved property type changed'));
    const pending = deferred<ScenePropertiesSnapshot>();
    vi.mocked(ipc.loadSceneProperties).mockReturnValueOnce(pending.promise);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Saved property type changed');
    panel.set({ disabled: true });
    await tick();
    expect(button(panel.target, 'Reset defaults').disabled).toBe(true);
    button(panel.target, 'Reset defaults').click();
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
    panel.set({ disabled: false });
    await tick();
    button(panel.target, 'Retry').click();
    await tick();
    expect(panel.target.querySelector('section')?.getAttribute('aria-busy')).toBe('true');
    expect(panel.target.textContent).not.toContain('Reset defaults');
    expect(ipc.updateSceneProperties).not.toHaveBeenCalled();
    pending.reject(new Error('Saved property type still invalid'));
    await waitForText(panel.target, 'Saved property type still invalid');
    expect(button(panel.target, 'Reset defaults').disabled).toBe(false);
  });

  it('reports a refresh failure after successful application and retries loading without another save', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValueOnce(fixture())
      .mockRejectedValueOnce(new Error('Refresh unavailable')).mockResolvedValueOnce(fixture());
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue(outcome({ message: 'Properties applied' }));
    panel = await mountPanel();
    await open();
    await edit('number', '4');
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Refresh unavailable');
    expect(panel.target.textContent).toContain('Properties applied');
    expect(field('number').value).toBe('4');
    button(panel.target, 'Retry refresh').click();
    await vi.waitFor(() => expect(panel!.target.textContent).not.toContain('Refresh unavailable'));
    expect(ipc.updateSceneProperties).toHaveBeenCalledTimes(1);
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(3);
  });

  it('rejects a truthy non-boolean outcome instead of claiming success', async () => {
    vi.mocked(ipc.updateSceneProperties).mockResolvedValue({ ...outcome(), ok: 'true' } as unknown as ActionOutcome<null>);
    panel = await mountPanel();
    await open();
    await edit('number', '4');
    button(panel.target, 'Save and apply').click();
    await waitForText(panel.target, 'Invalid action result');
    expect(field('number').value).toBe('4');
    expect(onOutcome).not.toHaveBeenCalled();
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
  });

  it('locks controls and prevents repeated submissions while applying', async () => {
    const pending = deferred<ActionOutcome<null>>();
    vi.mocked(ipc.updateSceneProperties).mockReturnValue(pending.promise);
    panel = await mountPanel();
    await open();
    button(panel.target, 'Save and apply').click();
    await tick();
    expect([...panel.target.querySelectorAll<HTMLInputElement>('input')].every((input) => input.disabled)).toBe(true);
    expect(button(panel.target, 'Reset defaults').disabled).toBe(true);
    button(panel.target, 'Saving and applying…').click();
    expect(ipc.updateSceneProperties).toHaveBeenCalledTimes(1);
    pending.resolve(outcome());
    await vi.waitFor(() => expect(button(panel!.target, 'Save and apply').disabled).toBe(false));
    panel.set({ disabled: true });
    await tick();
    expect(button(panel.target, 'Save and apply').disabled).toBe(true);
    expect(button(panel.target, 'Scene properties').disabled).toBe(true);
  });

  it.each(['late success', 'late rejection'])('ignores a previous monitor load after switching: %s', async (kind) => {
    const previous = deferred<ScenePropertiesSnapshot>();
    const current = fixture('DP-2');
    current.properties[0].label = 'Second monitor author label';
    vi.mocked(ipc.loadSceneProperties).mockReturnValueOnce(previous.promise).mockResolvedValueOnce(current);
    panel = await mountPanel();
    button(panel.target, 'Scene properties').click();
    await tick();
    panel.set({ monitorId: 'DP-2' });
    await tick();
    expect(button(panel.target, 'Scene properties').getAttribute('aria-expanded')).toBe('false');
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
    button(panel.target, 'Scene properties').click();
    await waitForText(panel.target, 'Second monitor author label');
    if (kind === 'late success') previous.resolve(fixture());
    else previous.reject(new Error('Old monitor error'));
    await tick();
    await tick();
    expect(panel.target.textContent).toContain('Second monitor author label');
    expect(panel.target.textContent).not.toContain('Author HDR label');
    expect(panel.target.textContent).not.toContain('Old monitor error');
    expect(ipc.loadSceneProperties).toHaveBeenLastCalledWith('DP-2', 'scene-1');
  });

  it('does not show an old save result after switching items, but forwards its completed mutation', async () => {
    const previous = deferred<ActionOutcome<null>>();
    vi.mocked(ipc.updateSceneProperties).mockReturnValue(previous.promise);
    panel = await mountPanel();
    await open();
    button(panel.target, 'Save and apply').click();
    await tick();
    panel.set({ itemId: 'scene-2' });
    await tick();
    previous.resolve(outcome({ message: 'Old item saved', invalidations: ['desktop', 'library'] }));
    await tick();
    await tick();
    expect(panel.target.textContent).not.toContain('Old item saved');
    expect(ipc.loadSceneProperties).toHaveBeenCalledTimes(1);
    expect(onOutcome).toHaveBeenCalledTimes(1);
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(fixture('DP-1', 'scene-2'));
    await open();
    expect(ipc.loadSceneProperties).toHaveBeenLastCalledWith('DP-1', 'scene-2');
  });

  it('shows bilingual control copy while retaining author-provided labels', async () => {
    vi.mocked(ipc.loadSceneProperties).mockResolvedValue(groupedFixture());
    setPreferredLanguage('zh-CN');
    panel = await mountPanel();
    button(panel.target, '场景属性').click();
    await waitForText(panel.target, 'Author brightness');
    expect(button(panel.target, '保存并应用')).toBeDefined();
    expect(button(panel.target, '恢复默认值')).toBeDefined();
    expect(panel.target.textContent).toContain('所选显示器');
    expect(panel.target.textContent).toContain('作者颜色');
    expect(panel.target.textContent).toContain('Author visuals');
    expect(panel.target.textContent).toContain('属性分组 2');
    expect(panel.target.textContent).toContain('此分组没有可编辑属性');
  });
});
