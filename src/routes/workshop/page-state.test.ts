import { describe, expect, it } from 'vitest';

import {
  isLatestWorkshopOnlineSearchResponse,
  isMissingSteamApiKeyError,
  nextWorkshopOnlineSearchPage,
  onlineRuntimeStatusKey,
  resolveWorkshopRefreshState
} from './page-state';

describe('workshop refresh state', () => {
  it('preserves a newer in-flight selection during refresh when the item still exists', () => {
    expect(
      resolveWorkshopRefreshState({
        currentSelection: 'item-b',
        hasCurrentUpdate: true,
        availableItemIds: ['item-a', 'item-b'],
        detailLoading: true,
        detailRequestToken: 4,
        detailError: 'Detail request failed'
      })
    ).toEqual({
      nextSelection: 'item-b',
      detailLoading: true,
      detailRequestToken: 4,
      detailError: 'Detail request failed'
    });
  });

  it('clears selection and resets detail state when the latest selection disappears after refresh', () => {
    expect(
      resolveWorkshopRefreshState({
        currentSelection: 'item-b',
        hasCurrentUpdate: true,
        availableItemIds: ['item-a'],
        detailLoading: true,
        detailRequestToken: 4,
        detailError: 'Detail request failed'
      })
    ).toEqual({
      nextSelection: null,
      detailLoading: false,
      detailRequestToken: 5,
      detailError: null
      });
  });

  it('clears selection and detail state when refresh succeeds without a current snapshot update', () => {
    expect(
      resolveWorkshopRefreshState({
        currentSelection: 'item-b',
        hasCurrentUpdate: false,
        availableItemIds: [],
        detailLoading: true,
        detailRequestToken: 4,
        detailError: 'Detail request failed'
      })
    ).toEqual({
      nextSelection: null,
      detailLoading: false,
      detailRequestToken: 5,
      detailError: null
    });
  });
});

describe('workshop online search token guard', () => {
  it('accepts only the latest response token', () => {
    expect(
      isLatestWorkshopOnlineSearchResponse({
        requestToken: 4,
        responseToken: 4
      })
    ).toBe(true);

    expect(
      isLatestWorkshopOnlineSearchResponse({
        requestToken: 3,
        responseToken: 4
      })
    ).toBe(false);
  });
});

describe('workshop online pagination helper', () => {
  it('increments page when more results are available', () => {
    expect(nextWorkshopOnlineSearchPage({ currentPage: 2, hasMore: true })).toBe(3);
  });

  it('keeps page unchanged when no more results are available', () => {
    expect(nextWorkshopOnlineSearchPage({ currentPage: 2, hasMore: false })).toBe(2);
  });
});

describe('workshop online clarity helpers', () => {
  it('detects missing Steam API key errors for Settings guidance', () => {
    expect(
      isMissingSteamApiKeyError(
        'Steam Web API key is required for online Workshop search. Add it in Settings.'
      )
    ).toBe(true);
    expect(isMissingSteamApiKeyError('Steam Workshop QueryFiles returned an error')).toBe(false);
    expect(isMissingSteamApiKeyError(null)).toBe(false);
  });

  it('maps online item types to honest runtime labels', () => {
    expect(onlineRuntimeStatusKey('video')).toBe('runnable');
    expect(onlineRuntimeStatusKey('scene')).toBe('recognizedOnly');
    expect(onlineRuntimeStatusKey('web')).toBe('recognizedOnly');
    expect(onlineRuntimeStatusKey('application')).toBe('unsupported');
  });
});
