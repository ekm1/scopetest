import { currency } from '@acme/utils';

export function renderTotal(cents: number): string {
  return `Total: ${currency(cents)}`;
}
