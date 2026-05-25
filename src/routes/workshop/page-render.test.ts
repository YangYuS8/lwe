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

  it('renders local marker controls without duplicating the local Library gallery', () => {
    const { body } = render(WorkshopPage);

    expect(body).toContain('Steam Workshop discovery');
    expect(body).toContain('Search Steam Workshop online');
    expect(body).toContain('Local sync markers');
    expect(body).toContain('Library owns local content.');
    expect(body).toContain('Refresh Catalog');
    expect(body).toContain('Open Library');
    expect(body).toContain('Online search');
    expect(body).not.toContain('Select a Workshop item to inspect its current detail payload.');
  });

  it('renders route and detail placeholder copy in Simplified Chinese when zh-CN is active', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(WorkshopPage);

    expect(body).toContain('创意工坊');
    expect(body).toContain('Steam 创意工坊发现');
    expect(body).toContain('本地同步标记');
    expect(body).toContain('在线搜索');
    expect(body).toContain('显示筛选');
    expect(body).not.toContain('工坊详情');
  });

  it('marks online results that already exist locally without rendering local item cards', () => {
    pageCache.set({
      library: {
        snapshot: {
          selectedItemId: null,
          monitorsAvailable: true,
          desktopAssignmentsAvailable: true,
          stale: false,
          items: [
            {
              id: 'workshop-1001',
              workshopId: '1001',
              title: 'Synced Video',
              itemType: 'video',
              coverPath: null,
              ageRating: 'g',
              source: 'workshop',
              compatibility: {
                badge: 'fully_supported',
                reasonCode: 'ready_for_library',
                summaryCopy: 'Ready to use'
              },
              applySupported: true,
              favorite: false,
              assignedMonitorLabels: []
            }
          ]
        },
        detail: null,
        stale: false
      },
      workshop: {
        snapshot: {
          selectedItemId: null,
          stale: false,
          items: [
            {
              id: '1001',
              title: 'Synced Video',
              itemType: 'video',
              coverPath: null,
              syncStatus: 'synced',
              compatibility: {
                badge: 'fully_supported',
                reasonCode: 'ready_for_library',
                summaryCopy: 'Ready to use'
              }
            }
          ]
        },
        detail: null,
        stale: false
      },
      desktop: { snapshot: null, detail: null, stale: false },
      settings: { snapshot: null, detail: null, stale: false }
    });
    setWorkshopOnlineCache({
      query: 'ambient',
      ageRatings: ['g'],
      itemTypes: ['video'],
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
            title: 'Online Synced Video',
            previewUrl: null,
            tags: ['video'],
            itemType: 'video',
            ageRating: 'g',
            ageRatingReason: 'No mature or explicit content markers were detected'
          },
          {
            id: '1002',
            title: 'Online Only Video',
            previewUrl: null,
            tags: ['video'],
            itemType: 'video',
            ageRating: 'g',
            ageRatingReason: 'No mature or explicit content markers were detected'
          }
        ]
      }
    });

    const { body } = render(WorkshopPage);

    expect(body).toContain('1 local Workshop items are available for online result markers.');
    expect(body).toContain('Online Synced Video');
    expect(body).toContain('Online Only Video');
    expect(body).toContain('In Library');
    expect(body).toContain('Online only');
    expect(body).not.toContain('Project metadata and the primary asset were found locally.');
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
