import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./en.json";
import zh from "./zh.json";

function detectLocale(): string {
  const saved = localStorage.getItem("loopsmith-locale");
  if (saved === "en" || saved === "zh") return saved;
  const nav = navigator.language.toLowerCase();
  if (nav.startsWith("zh")) return "zh";
  return "en";
}

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    zh: { translation: zh },
  },
  lng: detectLocale(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export function changeLocale(lng: string) {
  localStorage.setItem("loopsmith-locale", lng);
  i18n.changeLanguage(lng);
}

export default i18n;
