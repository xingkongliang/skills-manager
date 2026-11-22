/** The line diff fills a (left + 1) × (right + 1) table and renders every
 * changed row, so it is only computed within these bounds (#658); a larger
 * pair gets a one-line summary instead. Initial engineering budgets, not
 * measured limits. */
const MAX_DIFF_CELLS = 1_000_000;
const MAX_DIFF_LINES = 4_000;

export function diffExceedsBudget(original: string, updated: string): boolean {
  const left = original.split("\n").length;
  const right = updated.split("\n").length;
  return (left + 1) * (right + 1) > MAX_DIFF_CELLS || left + right > MAX_DIFF_LINES;
}
