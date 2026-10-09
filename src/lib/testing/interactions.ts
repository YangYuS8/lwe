import { mount, tick, unmount, type Component } from 'svelte';
import { expect, vi } from 'vitest';
import { pageCache, workshopOnlineCache } from '$lib/stores/ui';
import { resetPreferredLanguage } from '$lib/i18n';

export const mountPage = async (component: Component) => {
  const target = document.createElement('div');
  document.body.append(target);
  const instance = mount(component, { target });
  await tick();
  return {
    target,
    destroy: async () => { await unmount(instance); target.remove(); }
  };
};

export const button = (target: HTMLElement, label: string) => {
  const result = Array.from(target.querySelectorAll('button')).find((element) =>
    element.textContent?.trim() === label || element.getAttribute('aria-label') === label
  );
  expect(result, `Button ${label} must be present`).toBeDefined();
  return result!;
};

export const waitForText = async (target: HTMLElement, text: string) => {
  await vi.waitFor(() => expect(target.textContent).toContain(text));
};

export const resetPageStores = () => {
  pageCache.set({
    library: { snapshot: null, detail: null, stale: false },
    workshop: { snapshot: null, detail: null, stale: false },
    desktop: { snapshot: null, detail: null, stale: false },
    settings: { snapshot: null, detail: null, stale: false }
  });
  workshopOnlineCache.set({ query: '', ageRatings: ['g'], itemTypes: ['video'], pageSize: 24, result: null });
  resetPreferredLanguage();
};

export const deferred = <T>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
};
