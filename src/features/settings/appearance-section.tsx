"use client";

import { Segmented } from "@/components/ui/segmented";
import { Select } from "@/components/ui/input";
import { APPEARANCES, useAppearance, type Appearance } from "@/lib/appearance";
import { LOCALES, LOCALE_NAMES, getLocalePreference, setLocalePreference, useLocale, useT, type Locale } from "@/lib/i18n";
import { Row, Section, useSavedFlash } from "@/features/settings/settings-layout";

/** Theme (Dark / Black / Light) and language. Both apply at once here and in every other window. */
export function AppearanceSection() {
  const t = useT();
  const [mode, setMode] = useAppearance();
  useLocale();
  const language = getLocalePreference();
  const [saved, flash] = useSavedFlash();

  return (
    <Section title={t("settings.appearance.title")} description={t("settings.appearance.description")} saved={saved}>
      <Row
        title={t("settings.appearance.theme")}
        description={t(`settings.appearance.themeDescriptions.${mode}`)}
        control={
          <Segmented<Appearance>
            label={t("settings.appearance.theme")}
            value={mode}
            options={APPEARANCES.map((a) => ({ value: a.value, label: t(`settings.appearance.themes.${a.value}`) }))}
            onChange={(next) => {
              if (next === mode) return;
              setMode(next);
              flash();
            }}
          />
        }
      />
      <Row
        title={t("settings.appearance.language")}
        description={t("settings.appearance.languageDescription")}
        control={
          <Select
            fieldSize="sm"
            aria-label={t("settings.appearance.language")}
            value={language}
            onChange={(e) => {
              const next = e.target.value as Locale | "system";
              if (next === language) return;
              setLocalePreference(next);
              flash();
            }}
          >
            <option value="system">{t("settings.appearance.languageSystem")}</option>
            {LOCALES.map((l) => (
              <option key={l} value={l}>
                {LOCALE_NAMES[l]}
              </option>
            ))}
          </Select>
        }
      />
    </Section>
  );
}
