import { Button } from '@shop/ui';
import { total, type CartLine } from '@shop/data';

export function renderCheckout(lines: CartLine[]): string {
  return `${total(lines)} items ${Button('pay')}`;
}
