export interface CartLine {
  sku: string;
  qty: number;
}

export function total(lines: CartLine[]): number {
  return lines.reduce((sum, line) => sum + line.qty, 0);
}
