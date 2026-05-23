import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import WorkshopDetailPanel from './WorkshopDetailPanel.svelte';
import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';

describe('WorkshopDetailPanel', () => {
  afterEach(() => {
    resetPreferredLanguage();
  });

  it('uses a compact vertical detail structure for populated detail', () => {
    const { body } = render(WorkshopDetailPanel, {
      props: {
        detail: {
          id: 'workshop-1',
          title: 'Workshop Forest',
          itemType: 'scene',
          coverPath: null,
          syncStatus: 'synced',
          compatibility: {
            badge: 'fully_supported',
            reasonCode: 'ready_for_library',
            summaryCopy: 'Ready to use',
            headline: 'Ready to use',
            detail: 'This Workshop item is synchronized locally and ready to use.',
            nextStep: 'none',
            nextStepCopy: null
          },
          description: 'Dense detail flow.',
          tags: ['forest']
        },
        loading: false,
        error: null,
        openInSteam: async () => {}
      }
    });

    expect(body).toContain('data-detail-layout="compact-vertical"');
    expect(body).toContain('data-detail-section="header"');
    expect(body).toContain('data-detail-section="quick-status"');
    expect(body).toContain('data-detail-section="actions"');
    expect(body).toContain('data-detail-section="cover"');
    expect(body).toContain('data-detail-section="compatibility"');
    expect(body).toContain('data-detail-section="metadata"');
    expect(body).not.toContain('lg:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]');
    expect(body.indexOf('data-detail-section="header"')).toBeLessThan(
      body.indexOf('data-detail-section="quick-status"')
    );
    expect(body.indexOf('data-detail-section="quick-status"')).toBeLessThan(
      body.indexOf('data-detail-section="actions"')
    );
    expect(body.indexOf('data-detail-section="actions"')).toBeLessThan(
      body.indexOf('data-detail-section="cover"')
    );
    expect(body.indexOf('data-detail-section="cover"')).toBeLessThan(
      body.indexOf('data-detail-section="compatibility"')
    );
    expect(body.indexOf('data-detail-section="compatibility"')).toBeLessThan(
      body.indexOf('data-detail-section="metadata"')
    );
  });

  it('localizes workshop sync status copy for populated detail in Simplified Chinese', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(WorkshopDetailPanel, {
      props: {
        detail: {
          id: 'workshop-2',
          title: 'Workshop Dunes',
          itemType: 'scene',
          coverPath: null,
          syncStatus: 'missing_asset',
          compatibility: {
            badge: 'partially_supported',
            reasonCode: 'needs_asset',
            summaryCopy: '需要补充资源',
            headline: '需要补充资源',
            detail: '缺少一项本地资源。',
            nextStep: 'none',
            nextStepCopy: null
          },
          description: 'Localized workshop detail.',
          tags: ['desert']
        },
        loading: false,
        error: null,
        openInSteam: async () => {}
      }
    });

    expect(body).toContain('同步状态： 缺少资源文件');
    expect(body).toContain('>缺少资源文件<');
    expect(body).toContain('>部分支持<');
    expect(body).toContain('无封面');
    expect(body).toContain('有封面时会在这里显示。');
    expect(body).toContain('打开 Steam 创意工坊页面');
    expect(body).not.toContain('partially_supported');
    expect(body).not.toContain('missing_asset');
    expect(body).not.toContain('打开源页面');
  });

  it('shows compatibility copy and web item type labels for runtime-unsupported web items', () => {
    const { body } = render(WorkshopDetailPanel, {
      props: {
        detail: {
          id: 'workshop-web-1',
          title: 'Interactive Rain Site',
          itemType: 'web',
          coverPath: null,
          syncStatus: 'synced',
          compatibility: {
            badge: 'partially_supported',
            reasonCode: 'recognized_but_runtime_unsupported',
            summaryCopy: 'Recognized for reporting only',
            headline: 'Runtime unsupported',
            detail:
              'LWE recognizes this web item for compatibility reporting, but the current verified runtime path only applies video wallpapers.',
            nextStep: 'wait_for_future_support',
            nextStepCopy: 'This item remains visible for reporting until web runtime support is available.'
          },
          description: 'Web wallpaper detail.',
          tags: ['web', 'interactive']
        },
        loading: false,
        error: null,
        openInSteam: async () => {}
      }
    });

    expect(body).toContain('>Partially Supported<');
    expect(body).toContain('>Web<');
    expect(body).toContain('Recognized for reporting only');
    expect(body).toContain('Runtime unsupported');
    expect(body).toContain('compatibility reporting');
    expect(body).toContain('This item remains visible for reporting until web runtime support is available.');
    expect(body).not.toContain('recognized_but_runtime_unsupported');
    expect(body).not.toContain('wait_for_future_support');
  });

  it('uses the shared subpanel treatment for the empty detail state', () => {
    const { body } = render(WorkshopDetailPanel, {
      props: {
        detail: null,
        loading: false,
        error: null,
        openInSteam: null
      }
    });

    expect(body).toContain('Select a Workshop item to inspect its current detail payload.');
    expect(body).toContain('lwe-subpanel');
    expect(body).toContain('border-dashed');
    expect(body).toContain('role="status"');
  });

  it('uses the shared subpanel treatment for the error detail state', () => {
    const { body } = render(WorkshopDetailPanel, {
      props: {
        detail: null,
        loading: false,
        error: 'Unable to complete the Workshop request.',
        openInSteam: null
      }
    });

    expect(body).toContain('Unable to complete the Workshop request.');
    expect(body).toContain('lwe-subpanel');
    expect(body).toContain('lwe-warning-banner lwe-wrap-safe');
    expect(body).toContain('Workshop detail');
  });
});
