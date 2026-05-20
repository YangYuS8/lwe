import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';

import CompatibilityPanel from './CompatibilityPanel.svelte';

describe('CompatibilityPanel', () => {
  it('renders the explanation headline and detail', () => {
    const { body } = render(CompatibilityPanel, {
      props: {
        compatibility: {
          badge: 'unsupported',
          reasonCode: 'unsupported_web_item',
          summaryCopy: 'Web support coming later',
          headline: 'Web item not in first release',
          detail: 'Web Workshop items are recognized, but not yet supported.',
          nextStep: 'wait_for_future_support',
          nextStepCopy: 'Support for this item is planned for a future update.'
        }
      }
    });

    expect(body).toContain('Web item not in first release');
    expect(body).toContain('Web Workshop items are recognized, but not yet supported.');
    expect(body).toContain('Next step: Support for this item is planned for a future update.');
    expect(body).not.toContain('unsupported_web_item');
    expect(body).not.toContain('wait_for_future_support');
    expect(body).toContain('data-slot="card"');
    expect(body).toContain('data-slot="separator"');
  });

  it('omits the next-step block when no assembled guidance copy is provided', () => {
    const { body } = render(CompatibilityPanel, {
      props: {
        compatibility: {
          badge: 'fully_supported',
          reasonCode: 'ready_for_library',
          summaryCopy: 'Ready to use',
          headline: 'Ready to use',
          detail: 'This video item is synchronized locally and available for Library and desktop use.',
          nextStep: 'none',
          nextStepCopy: null
        }
      }
    });

    expect(body).not.toContain('Next step:');
  });
});
