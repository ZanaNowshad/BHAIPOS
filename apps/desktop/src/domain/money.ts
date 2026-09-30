export type Fils = number & { readonly __brand: 'Fils' };
export const fils = (value: number): Fils => {
  if (!Number.isSafeInteger(value)) throw new Error('Fils must be a safe integer');
  return value as Fils;
};
export const addFils = (a: Fils, b: Fils): Fils => fils(a + b);
export const formatBhd = (value: Fils): string => {
  const sign = value < 0 ? '-' : '';
  const abs = Math.abs(value);
  return `${sign}${Math.floor(abs / 1000)}.${String(abs % 1000).padStart(3, '0')}`;
};

export const parseBhd = (input: string): Fils => {
  const match = input.trim().match(/^(\d+)(?:\.(\d{1,3}))?$/);
  if (!match) throw new Error('Enter a positive BHD amount with at most three decimals');
  const whole = Number(match[1]);
  const fraction = Number((match[2] ?? '').padEnd(3, '0'));
  return fils(whole * 1000 + fraction);
};

export const parseQuantityMilli = (input: string): number => {
  const match=input.trim().match(/^(\d+)(?:\.(\d{1,3}))?$/);
  if (!match) throw new Error('Enter a positive quantity with at most three decimals');
  const value=Number(match[1])*1000+Number((match[2]??'').padEnd(3,'0'));
  if (!Number.isSafeInteger(value)||value<=0) throw new Error('Quantity must be greater than zero');
  return value;
};
