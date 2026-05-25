import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import LibraryPage from './+page.svelte';
import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import { pageCache, setLibraryDetail, setLibrarySnapshot } from '$lib/stores/ui';

const resetCache = () => {
  pageCache.set({
    library: { snapshot: null, detail: null, stale: false },
    workshop: { snapshot: null, detail: null, stale: false },
    desktop: { snapshot: null, detail: null, stale: false },
    settings: { snapshot: null, detail: null, stale: false }
  });
};

describe('library page render', () => {
  afterEach(() => {
    resetPreferredLanguage();
    resetCache();
  });

  it('does not render the split body when no library snapshot is available', () => {
    const { body } = render(LibraryPage);

    expect(body).toContain('Your local library');
    expect(body).toContain('Browse the content you already own or have synchronized onto this machine.');
    expect(body).not.toContain('No Library items are available in the current snapshot.');
    expect(body).not.toContain('Select a Library item to inspect its current detail payload.');
  });

  it('renders assigned monitor labels on cards and in the detail panel', () => {
    setLibrarySnapshot({
      items: [
        {
          id: 'video-7',
          title: 'Forest Video',
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
          assignedMonitorLabels: ['Primary', 'DISPLAY-2 (missing)']
        }
      ],
      selectedItemId: 'video-7',
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      stale: false
    });

    setLibraryDetail({
      id: 'video-7',
      title: 'Forest Video',
      itemType: 'video',
      coverPath: null,
      source: 'workshop',
      compatibility: {
        badge: 'fully_supported',
        reasonCode: 'ready_for_library',
        summaryCopy: 'Ready to use',
        headline: 'Ready to use',
        detail: 'This video item is synchronized locally and available for Library and desktop use.',
        nextStep: 'none',
        nextStepCopy: null
      },
      applySupported: true,
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      assignedMonitorLabels: ['Primary', 'DISPLAY-2 (missing)'],
      description: null,
      tags: []
    });

    const { body } = render(LibraryPage);

    expect(body).toContain('Assigned to');
    expect(body).toContain('Assigned monitors');
    expect(body).toContain('Primary');
    expect(body).toContain('DISPLAY-2 (missing)');
  });

  it('makes runnable and recognized-only Library items distinguishable from cards', () => {
    setLibrarySnapshot({
      items: [
        {
          id: 'video-7',
          title: 'Forest Video',
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
        },
        {
          id: 'scene-8',
          title: 'Forest Scene',
          itemType: 'scene',
          coverPath: null,
          ageRating: 'g',
          source: 'workshop',
          compatibility: {
            badge: 'partially_supported',
            reasonCode: 'recognized_runtime_unsupported',
            summaryCopy: 'Recognized only'
          },
          applySupported: false,
          favorite: false,
          assignedMonitorLabels: []
        }
      ],
      selectedItemId: null,
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      stale: false
    });

    const { body } = render(LibraryPage);

    expect(body).toContain('Forest Video');
    expect(body).toContain('Runnable video');
    expect(body).toContain('Forest Scene');
    expect(body).toContain('Recognized only');
    expect(body).toContain('not runnable by the current video runtime');
  });

  it('renders route and detail placeholder copy in Simplified Chinese when zh-CN is active', () => {
    setPreferredLanguage('zh-CN');

    setLibrarySnapshot({
      items: [],
      selectedItemId: null,
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      stale: false
    });

    const { body } = render(LibraryPage);

    expect(body).toContain('内容库');
    expect(body).toContain('本地内容库');
    expect(body).toContain('浏览这台设备上已经拥有或已同步的内容。');
    expect(body).toContain('当前快照中没有可用的内容项。');
    expect(body).toContain('内容详情');
    expect(body).toContain('选择一个内容项以查看当前详情。');
  });

  it('renders shared Library detail labels in Simplified Chinese when zh-CN is active', () => {
    setPreferredLanguage('zh-CN');

    setLibrarySnapshot({
      items: [
        {
          id: 'video-7',
          title: 'Forest Video',
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
      ],
      selectedItemId: 'video-7',
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      stale: false
    });

    setLibraryDetail({
      id: 'video-7',
      title: 'Forest Video',
      itemType: 'video',
      coverPath: null,
      source: 'workshop',
      compatibility: {
        badge: 'fully_supported',
        reasonCode: 'ready_for_library',
        summaryCopy: 'Ready to use',
        headline: 'Ready to use',
        detail: 'This video item is synchronized locally and available for Library and desktop use.',
        nextStep: 'resync_workshop_item',
        nextStepCopy: 'Choose a monitor before applying this item.'
      },
      applySupported: true,
      monitorsAvailable: true,
      monitorDiscoveryIssue: null,
      desktopAssignmentIssue: null,
      desktopAssignmentsAvailable: true,
      assignedMonitorLabels: [],
      description: null,
      tags: []
    });

    const { body } = render(LibraryPage);

    expect(body).toContain('aria-label="选择内容项 Forest Video"');
    expect(body).not.toContain('显示 Forest Video 的快捷操作');
    expect(body).toContain('兼容性');
    expect(body).toContain('下一步：');
    expect(body).toContain('完全支持');
    expect(body).not.toContain('Fully Supported');
    expect(body).toContain('Choose a monitor before applying this item.');
  });
});
