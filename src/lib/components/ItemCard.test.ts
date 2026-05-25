import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import ItemCard from './ItemCard.svelte';

describe('ItemCard', () => {
  afterEach(() => {
    resetPreferredLanguage();
  });

  it('renders the assembled compatibility summary copy', () => {
    const { body } = render(ItemCard, {
      props: {
        title: 'Forest Scene',
        coverPath: null,
        selected: false,
        assignedMonitorLabels: [],
        itemType: 'video',
        compatibility: {
          badge: 'fully_supported',
          reasonCode: 'ready_for_library',
          summaryCopy: 'Ready to use'
        },
        applySupported: true,
        selectLabel: 'Select Forest Scene',
        onSelect: () => {}
      }
    });

    expect(body).toContain('data-slot="card"');
    expect(body).toContain('Video');
    expect(body).toContain('Fully Supported');
    expect(body).toContain('Runnable video');
    expect(body).toContain('Select Forest Scene');
    expect(body).toContain('aria-pressed="false"');
  });

  it('explains recognized-only items from the card surface', () => {
    const { body } = render(ItemCard, {
      props: {
        title: 'Forest Scene',
        coverPath: null,
        selected: false,
        assignedMonitorLabels: [],
        itemType: 'scene',
        compatibility: {
          badge: 'partially_supported',
          reasonCode: 'recognized_runtime_unsupported',
          summaryCopy: 'Recognized only'
        },
        applySupported: false
      }
    });

    expect(body).toContain('Scene');
    expect(body).toContain('Partially Supported');
    expect(body).toContain('Recognized only');
    expect(body).toContain('not runnable by the current video runtime');
  });

  it('renders assigned monitor labels from the assembled library quick status', () => {
    const { body } = render(ItemCard, {
      props: {
        title: 'Forest Scene',
        coverPath: null,
        selected: true,
        assignedMonitorLabels: ['Primary', 'DISPLAY-2 (missing)']
      }
    });

    expect(body).toContain('Assigned to');
    expect(body).toContain('Primary');
    expect(body).toContain('DISPLAY-2 (missing)');
  });

  it('exposes a selectable action path on the item surface', () => {
    const { body } = render(ItemCard, {
      props: {
        title: 'Forest Scene',
        coverPath: null,
        selected: false,
        assignedMonitorLabels: [],
        selectLabel: 'Select Forest Scene',
        onSelect: () => {}
      }
    });

    expect(body).toContain('Select Forest Scene');
    expect(body).toContain('aria-pressed="false"');
  });

  it('localizes assigned monitor heading from centralized i18n copy', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(ItemCard, {
      props: {
        title: 'Forest Scene',
        coverPath: null,
        selected: false,
        assignedMonitorLabels: ['主显示器'],
        itemType: 'video',
        applySupported: true
      }
    });

    expect(body).toContain('已分配到');
    expect(body).toContain('可运行视频');
    expect(body).toContain('主显示器');
    expect(body).not.toContain('Assigned to');
  });
});
