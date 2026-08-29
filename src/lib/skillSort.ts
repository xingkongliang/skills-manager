/**
 * Case-insensitive, numeric-aware locale comparison. This is what "dictionary
 * order" means for skill lists: `a2` sorts before `a10`, and "Zebra" next to
 * "apple" is treated by the letter rather than by ASCII case.
 */
export function compareSkillNames(a: string, b: string): number {
  return a.localeCompare(b, undefined, { sensitivity: "base", numeric: true });
}

/**
 * Returns a new array sorted by dictionary order of the name returned by
 * `getName`. The input array is left untouched.
 */
export function sortSkillsByName<T>(
  skills: readonly T[],
  getName: (skill: T) => string,
): T[] {
  return [...skills].sort((a, b) => compareSkillNames(getName(a), getName(b)));
}
