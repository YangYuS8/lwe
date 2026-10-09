import type { ActionOutcome } from '$lib/types';

export const actionOutcomeError = (outcome: ActionOutcome<unknown>, fallback: string): string | null =>
  outcome.ok ? null : outcome.message || fallback;
