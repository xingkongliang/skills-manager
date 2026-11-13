export const readerLanguages = { "zh-CN": "简体中文", "zh-TW": "繁體中文", en: "English", es: "Español", ko: "한국어" } as const;
export type ReaderLanguage = keyof typeof readerLanguages;
export function targetLanguage(locale: string): ReaderLanguage {
  if (/^zh/i.test(locale)) return /hant|tw|hk|mo/i.test(locale) ? "zh-TW" : "zh-CN";
  return /^ko/i.test(locale) ? "ko" : /^es/i.test(locale) ? "es" : "en";
}
// Inspect prose only; callers remove code, frontmatter and protected tokens.
export function proseLanguage(text: string): string | null {
  const prose = text.replace(/⟦S\d+⟧/g, "");
  const han = (prose.match(/[\u3400-\u9fff]/g) ?? []).length;
  const hangul = (prose.match(/[\uac00-\ud7af]/g) ?? []).length;
  const latin = (prose.match(/[a-zA-Záéíóúñü]/g) ?? []).length;
  if (hangul > 0 && hangul * 3 >= latin) return "ko";
  if (han > 0 && han * 3 >= latin) return "zh";
  if (!latin) return null;
  return /[¿¡ñ]|\b(el|los|las|para|una|del|que|archivo|utilizar)\b/i.test(prose) ? "es" : "en";
}
export function needsTranslation(text: string, target: string): boolean {
  const source = proseLanguage(text);
  return source !== null && source !== target.split("-")[0];
}
