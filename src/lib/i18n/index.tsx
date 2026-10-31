import { createContext, Fragment, useContext, useEffect, type ReactNode } from "react";
import type { Lang } from "../api";
import { formattersFor, type Fmt } from "../format";
import { en } from "./en";
import { id, type Key } from "./id";

export type { Key } from "./id";
export type { Lang } from "../api";
export type { Fmt } from "../format";

export const DICTIONARIES: Record<Lang, Record<Key, string>> = { id, en };

type Vars = Record<string, string | number>;

export interface T {
  (key: Key, vars?: Vars): string;
  /** Like `t`, but `<b>..</b>` in the text becomes bold. Values go in as text, never as markup. */
  rich: (key: Key, vars?: Vars) => ReactNode;
}

const fill = (text: string, vars?: Vars): string => text.replace(/\{(\w+)\}/g, (_, name: string) => String(vars?.[name] ?? ""));

function makeT(lang: Lang): T {
  const dict = DICTIONARIES[lang];
  const t = ((key: Key, vars?: Vars) => fill(dict[key], vars)) as T;
  t.rich = (key, vars) =>
    // Split on the template, so a value holding "<b>" cannot change the markup.
    dict[key].split(/<b>(.*?)<\/b>/).map((part, i) =>
      i % 2 === 1 ? (
        <b key={i}>{fill(part, vars)}</b>
      ) : (
        <Fragment key={i}>{fill(part, vars)}</Fragment>
      ),
    );
  return t;
}

const translators: Record<Lang, T> = { id: makeT("id"), en: makeT("en") };

/** Plain function for code outside React (tests, helpers). */
export const translator = (lang: Lang): T => translators[lang];

const LangContext = createContext<Lang>("id");

/** Everything below it reads its texts, numbers and dates in `lang`. Without a provider it is Indonesian. */
export function I18nProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  return <LangContext.Provider value={lang}>{children}</LangContext.Provider>;
}

export const useLang = (): Lang => useContext(LangContext);

/** The dictionary lookup for the current language. */
export const useT = (): T => translators[useContext(LangContext)];

/** Number and date formatters for the current language. */
export const useFmt = (): Fmt => formattersFor(useContext(LangContext));

export const useI18n = (): { lang: Lang; t: T; f: Fmt } => {
  const lang = useContext(LangContext);
  return { lang, t: translators[lang], f: formattersFor(lang) };
};
