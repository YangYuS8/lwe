import { afterEach, describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import { resetPreferredLanguage, setPreferredLanguage } from '$lib/i18n';
import DesktopMonitorCard from './DesktopMonitorCard.svelte';

describe('DesktopMonitorCard', () => {
  afterEach(() => {
    resetPreferredLanguage();
  });

  it('renders media and restore state through the shared product card structure', () => {
    const { body } = render(DesktopMonitorCard, {
      props: {
        displayName: 'Primary',
        monitorId: 'DISPLAY-1',
        resolution: '1920x1080',
        currentItemLabel: 'Forest Scene',
        currentCoverPath: '/covers/forest-scene.jpg',
        runtimeStatus: 'unsupported',
        restoreState: 'restored',
        restoreIssue: 'Saved assignment was restored from the last session.',
        missing: false
      }
    });

    expect(body).toContain('Primary');
    expect(body).toContain('DISPLAY-1');
    expect(body).toContain('1920x1080');
    expect(body).toContain('Forest Scene');
    expect(body).toContain('Unsupported');
    expect(body).toContain('Restored');
    expect(body).toContain('Primary current item');
    expect(body).toContain('Restore state');
    expect(body).toContain('Saved assignment was restored from the last session.');
    expect(body).toContain('data-slot="card"');
    expect(body).toContain('View status details');
    expect(body).toContain('aria-expanded="false"');
    expect(body).toContain('lwe-warning-banner');
  });

  it('renders zh-CN-owned labels without falling back to English badge copy', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(DesktopMonitorCard, {
      props: {
        displayName: 'Primary',
        monitorId: 'DISPLAY-1',
        resolution: '1920x1080',
        currentItemLabel: '没有已保存的分配',
        currentCoverPath: null,
        runtimeStatus: 'unsupported',
        restoreState: 'restored',
        restoreIssue: 'Saved assignment was restored from the last session.',
        missing: false
      }
    });

    expect(body).toContain('桌面显示器 Primary');
    expect(body).toContain('桌面');
    expect(body).toContain('当前内容');
    expect(body).toContain('恢复状态');
    expect(body).not.toContain('>Desktop<');
  });

  it('omits the large no-cover artwork placeholder when no monitor cover is available', () => {
    const { body } = render(DesktopMonitorCard, {
      props: {
        displayName: 'Primary',
        monitorId: 'DISPLAY-1',
        resolution: '1920x1080',
        currentItemLabel: 'No saved assignment',
        currentCoverPath: null,
        runtimeStatus: 'idle',
        restoreState: null,
        restoreIssue: null,
        missing: false
      }
    });

    expect(body).toContain('Primary');
    expect(body).toContain('No saved assignment');
    expect(body).not.toContain('No Cover');
    expect(body).not.toContain('Artwork will appear here when available.');
  });

  it('localizes frontend-owned runtime and restore badges through centralized copy', () => {
    setPreferredLanguage('zh-CN');

    const { body } = render(DesktopMonitorCard, {
      props: {
        displayName: 'Primary',
        monitorId: 'DISPLAY-1',
        resolution: '1920x1080',
        currentItemLabel: '没有已保存的分配',
        currentCoverPath: null,
        runtimeStatus: 'running',
        restoreState: 'missing_item',
        restoreIssue: 'Saved assignment still points to an item that is no longer available.',
        missing: false
      }
    });

    expect(body).toContain('运行中');
    expect(body).toContain('缺少内容项');
    expect(body).not.toContain('Running');
    expect(body).not.toContain('Missing Item');
  });

  it('renders the missing monitor badge only once when the card is already marked missing', () => {
    const { body } = render(DesktopMonitorCard, {
      props: {
        displayName: 'Detached Display',
        monitorId: 'DISPLAY-2',
        resolution: null,
        currentItemLabel: 'Ocean Scene',
        currentCoverPath: null,
        restoreState: 'missing_monitor',
        restoreIssue: 'Saved assignment targets a monitor that is not currently available.',
        missing: true
      }
    });

    expect(body.match(/Missing Monitor/g)).toHaveLength(2);
  });
});
