import { createRawSnippet } from 'svelte';
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import AppShell from './AppShell.svelte';

describe('AppShell', () => {
  it('renders persistent navigation, current-page emphasis, and a main region', () => {
    const children = createRawSnippet(() => ({
      render: () => '<section data-testid="shell-content">Shell content</section>'
    }));

    const { body } = render(AppShell, {
      props: {
        currentPath: '/desktop',
        children
      }
    });

    expect(body).toContain('Library');
    expect(body).toContain('Workshop');
    expect(body).toContain('Desktop');
    expect(body).toContain('Settings');
    expect(body).toContain('aria-current="page"');
    expect(body).toMatch(
      /<main class="lwe-shell-main" id="app-content" tabindex="-1"><section data-testid="shell-content">Shell content<\/section><!----><\/main>/
    );
  });

  it('renders navigation in Simplified Chinese when preferredLanguage is zh-CN', () => {
    const { body } = render(AppShell, {
      props: {
        currentPath: '/desktop',
        preferredLanguage: 'zh-CN'
      }
    });

    expect(body).toContain('内容库');
    expect(body).toContain('创意工坊');
    expect(body).toContain('桌面');
    expect(body).toContain('设置');
    expect(body).toContain('查看本地内容与当前应用状态。');
    expect(body).toContain('同步 Steam 创意工坊项目并刷新目录。');
    expect(body).toContain('查看显示器输出和恢复状态。');
    expect(body).toContain('调整界面语言、主题与启动行为。');
  });

  it('renders the shell with the shared utility-based structure and skip link wiring', () => {
    const { body } = render(AppShell, {
      props: {
        currentPath: '/library'
      }
    });

    expect(body).toContain('Skip to content');
    expect(body).toContain('lwe-titlebar');
    expect(body).toContain('data-tauri-drag-region');
    expect(body).toContain('href="#app-content"');
    expect(body).toContain('tabindex="-1"');
    expect(body).toContain('lwe-shell-sidebar');
    expect(body).toContain('lwe-shell-grid');
    expect(body).toContain('aria-label="Primary navigation"');
  });
});
