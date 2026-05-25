<script lang="ts">
  import { browser } from '$app/environment';
  import type { Snippet } from 'svelte';
  import { copy, getCopyForLanguage, type PreferredLanguage } from '$lib/i18n';
  import Titlebar from '$lib/layout/Titlebar.svelte';

  type NavItem = {
    href: string;
    key: 'library' | 'workshop' | 'desktop' | 'settings';
  };

  const navItems: NavItem[] = [
    {
      href: '/library',
      key: 'library'
    },
    {
      href: '/workshop',
      key: 'workshop'
    },
    {
      href: '/desktop',
      key: 'desktop'
    },
    {
      href: '/settings',
      key: 'settings'
    }
  ];

  let {
    currentPath = '/library',
    preferredLanguage = 'en',
    children
  }: {
    currentPath?: string;
    preferredLanguage?: PreferredLanguage;
    children?: Snippet;
  } = $props();

  const renderCopy = $derived(browser ? $copy : getCopyForLanguage(preferredLanguage));

  const isCurrent = (href: string, pathname: string) => pathname === href || pathname.startsWith(`${href}/`);

  const navLinkClass = (current: boolean) =>
    ['lwe-nav-link', current ? 'lwe-nav-link-active' : 'lwe-nav-link-idle'].join(' ');

</script>

<a class="lwe-skip-link" href="#app-content">{renderCopy.appShell.skipToContent}</a>

<div class="lwe-shell-bg">
  <Titlebar />
  <div class="lwe-shell-grid">
    <aside class="lwe-shell-sidebar" aria-label={renderCopy.appShell.primaryLandmark}>
      <div class="grid gap-2 pb-1">
        <p class="lwe-kicker">LWE</p>
        <p class="m-0 text-[1.35rem] font-semibold tracking-tight text-foreground">Wallpaper Engine</p>
        <p class="m-0 text-sm leading-6 text-muted-foreground">
          {renderCopy.appShell.appDescription}
        </p>
      </div>

      <nav class="grid gap-3" aria-label={renderCopy.appShell.primaryNavigation}>
        {#each navItems as item}
          {@const current = isCurrent(item.href, currentPath)}
          {@const section = renderCopy[item.key]}

          <a class={navLinkClass(current)} href={item.href} aria-current={current ? 'page' : undefined}>
            <span class="text-base font-semibold text-foreground">{section.navLabel}</span>
            <span class="text-[0.68rem] font-semibold uppercase tracking-[0.2em] text-muted-foreground">
              {section.navShortLabel}
            </span>
            <span class="text-sm leading-6 text-muted-foreground">{section.navDescription}</span>
          </a>
        {/each}
      </nav>
    </aside>

    <main class="lwe-shell-main" id="app-content" tabindex="-1">
      {@render children?.()}
    </main>
  </div>
</div>
