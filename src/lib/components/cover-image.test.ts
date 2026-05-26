import { describe, expect, it } from 'vitest';

import { resolveCoverSrc, shouldRenderCoverImage } from './cover-image';

describe('cover image fallback', () => {
  it('falls back to the placeholder after a load failure', () => {
    expect(shouldRenderCoverImage('/covers/item.jpg', false)).toBe(true);
    expect(shouldRenderCoverImage('/covers/item.jpg', true)).toBe(false);
  });

  it('uses the placeholder when there is no usable path', () => {
    expect(shouldRenderCoverImage(null, false)).toBe(false);
    expect(shouldRenderCoverImage('', false)).toBe(false);
  });

  it('keeps missing local covers as placeholders during server rendering', () => {
    expect(resolveCoverSrc('/home/user/.cache/lwe/card-thumbnails/item.webp')).toBeUndefined();
  });

  it('passes through already resolved asset URLs', () => {
    expect(resolveCoverSrc('asset://localhost/home/user/.cache/lwe/card-thumbnails/item.webp')).toBe(
      'asset://localhost/home/user/.cache/lwe/card-thumbnails/item.webp'
    );
  });
});
