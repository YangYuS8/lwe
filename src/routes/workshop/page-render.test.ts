import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import WorkshopPage from './+page.svelte';
import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import { pageCache, setWorkshopOnlineCache } from '$lib/stores/ui';

const resetCache = () => {
  pageCache.set({
    library: { snapshot: null, detail: null, stale: false },
    workshop: { snapshot: null, detail: null, stale: false },
    desktop: { snapshot: null, detail: null, stale: false },
    settings: { snapshot: null, detail: null, stale: false }
  });
};

describe('workshop page render', () => {
  afterEach(() => {
    resetPreferredLanguage();
    resetCache();
  });

  it('does not render the split body when no workshop snapshot is available', () => {
    const { body } = render(WorkshopPage);

    expect(body).toContain('Local Workshop sync');
    expect(body).toContain('Review local sync state and search Steam Workshop online with saved filters.');
    expect(body).toContain('Online search');
    expect(body).not.toContain('No Workshop items are available in the current snapshot.');
    expect(body).not.toContain('Select a Workshop item to inspect its current detail payload.');
  });

  it('renders route and detail placeholder copy in Simplified Chinese when zh-CN is active', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(WorkshopPage);

    expect(body).toContain('创意工坊');
    expect(body).toContain('本地创意工坊同步');
    expect(body).toContain('在线搜索');
    expect(body).toContain('显示筛选');
    expect(body).not.toContain('当前快照中没有可用的创意工坊项目。');
    expect(body).not.toContain('工坊详情');
  });

  it('labels scene and web online results as recognized only, not locally synced or runnable', () => {
    setWorkshopOnlineCache({
      query: 'ambient',
      ageRatings: ['g'],
      itemTypes: ['scene', 'web'],
      pageSize: 24,
      result: {
        query: 'ambient',
        page: 1,
        pageSize: 24,
        hasMore: false,
        totalApprox: 2,
        items: [
          {
            id: '1001',
            title: 'Ambient Scene',
            previewUrl: null,
            tags: ['scene'],
            itemType: 'scene',
            ageRating: 'g',
            ageRatingReason: 'No mature or explicit content markers were detected'
          },
          {
            id: '1002',
            title: 'Dashboard Web',
            previewUrl: null,
            tags: ['web'],
            itemType: 'web',
            ageRating: 'g',
            ageRatingReason: 'No mature or explicit content markers were detected'
          }
        ]
      }
    });

    const { body } = render(WorkshopPage);

    expect(body).toContain('Online results are discovery only.');
    expect(body).toContain('Ambient Scene');
    expect(body).toContain('Dashboard Web');
    expect(body).toContain('Recognized only');
    expect(body).toContain('not runnable by the current video runtime');
  });
});
